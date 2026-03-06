mod state;
mod audio_input;
mod audio_output;
mod export;
mod gui;

use std::sync::{Arc, Mutex};
use cpal::traits::StreamTrait;
use state::{RecorderState, Command, dispatch_command, PlaybackState};
use audio_output::{play_segment_async, play_project_async, ProjectSnapshot};
use colored::*;

// ** input **
// Microphone -> audio_input.rs ->(samples only)
// RecorderState.current.samples -> Approve → Project.segments
// -> export.rs → WAV
// 
// ** playback **
// Project / current segment -> (read-only) audio_output.rs
// -> Speakers
//
// user input should only affect RecorderState methods
// Initializes the RecorderState inside an Arc<Mutex<>>.
// start the input stream immediately (it's always listening, but only save when AppState is Recording)
//
// Workflow
// Idle: Type `r`. RecorderState creates a new empty Segment in current. State becomes Recording
// Recording: You speak. audio_input.rs wakes up repeatedly, locks the state, and pushes your voice data into current.samples
// Stop: Type `s`. State becomes Reviewing. The mic data stops being saved into the segment
// Review: Type `p`. main.rs unlocks the state, grabs the last segment, and sends it to audio_output.rs to play
// Decision:
// - Good: Type `c`. approve() moves current into project.segments. State becomes Idle
// - Bad: Type `x`. reject() deletes current. State becomes Idle. You can type r to try again
// Finish: Type `e`. export.rs combines all project.segments into one WAV file


// RecorderApp is the App struct which owns all long-lived resources
// and implements eframe::App
//
// Owned resources:
//   _stream:  must live as long as the app; dropping it silences the mic
//   recorder: Arc<Mutex<RecorderState>> shared with the audio thread
// `_stream` field name starts with `_` so Rust knows the drop is
// intentional (not a bug) and won't emit an unused-variable warning.
pub struct RecorderApp {
    recorder: Arc<Mutex<RecorderState>>,
    _stream: cpal::Stream, // keep alive for the entire app lifetime
}

impl RecorderApp {
    pub fn new(on_new_data: impl Fn() + Send + 'static) -> Self {
        // run_gui passes ctx.request_repaint(), while CLI passes || {}
        let recorder = Arc::new(Mutex::new(RecorderState::new(48000, 1)));
        let stream = audio_input::start_input_stream(recorder.clone(), on_new_data);
        stream.play().unwrap();
        Self { recorder, _stream: stream}
    }

    // rec.current which is the pending take that hasn't been approved yet, this is 
    // separate from PlaySegment(idx) because 'current' hasn't been commited to project.segments
    // used in auto-play on stop, and "listen again" during review
    fn play_current_segment(&self) {
        let rec = self.recorder.lock().unwrap();
        if rec.playback_state == PlaybackState::Playing { return; }
        if let Some(seg) = &rec.current { // current recording
            let seg_clone = seg.clone();
            let sample_rate = rec.project.sample_rate;
            drop(rec);
            let on_done = || {};
            play_segment_async(seg_clone, sample_rate, self.recorder.clone(), on_done);
        }
    }

    // PlaySegment / PlayAll / Export are handled here because they need either
    // thread-spawning (playback) or file I/O (export) — not pure state mutation.
    pub fn handle_command(&self, cmd: Command) {
        match cmd {
            Command::StopRecording => {
                {
                    let mut rec = self.recorder.lock().unwrap();
                    rec.stop_recording(); // change to reviewing
                }
                self.play_current_segment(); // auto-play after stopping
            }

            // *** dispatch commands

            Command::Approve => { // gated by playback state
                let rec = self.recorder.lock().unwrap();
                if rec.playback_state == PlaybackState::Playing {
                    println!("Wait for playback to finish before confirming.");
                    return;
                }
                drop(rec);
                let mut rec = self.recorder.lock().unwrap();
                dispatch_command(&mut rec, Command::Approve);
            }

            Command::Reject => { // gated by playback state
                let rec = self.recorder.lock().unwrap();
                if rec.playback_state == PlaybackState::Playing {
                    println!("Wait for playback to finish before rejecting.");
                    return;
                }
                drop(rec);
                let mut rec = self.recorder.lock().unwrap();
                dispatch_command(&mut rec, Command::Reject);
            }

            Command::RetryCurrentTake => {
                let rec = self.recorder.lock().unwrap();
                if rec.playback_state == PlaybackState::Playing {
                    println!( "Wait for playback to finish before retrying. ");
                    return;
                }
                drop(rec);
                let mut rec = self.recorder.lock().unwrap();
                dispatch_command(&mut rec, Command::RetryCurrentTake);
            }

            // *** Non-dispatch commands

            Command::PlaySegment(idx) => {
                let rec = self.recorder.lock().unwrap();
                if rec.playback_state == PlaybackState::Playing { return; } // already playing

                if let Some(seg) = rec.get_segment(idx) {
                    let seg_clone = seg.clone();
                    let sample_rate = rec.project.sample_rate;
                    drop(rec);

                    let on_done = || {};
                    play_segment_async(seg_clone, sample_rate, self.recorder.clone(), on_done);
                }
            }

            Command::PlayAll => {
                let rec = self.recorder.lock().unwrap();
                if rec.playback_state == PlaybackState::Playing { return; }
                if rec.project.segments.is_empty() { return; }

                let snapshot = ProjectSnapshot::from_project(&rec.project);
                drop(rec);

                let on_done = || {};
                play_project_async(snapshot, self.recorder.clone(), on_done);
            }

            Command::Export(path) => {
                let rec = self.recorder.lock().unwrap();
                export::export_wav(&rec.project, &path);
                println!("Exported to {}", path);
            }

            // All other commands change state.rs which are delegated to dispatch_command
            other => {
                let mut rec = self.recorder.lock().unwrap();
                dispatch_command(&mut rec, other);
            }

        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let use_gui = args.iter().any(|a| a == "--gui");
    if use_gui { run_gui(); } else { run_cli(); }
}

fn run_gui() {
}

fn run_cli() {
    let app = RecorderApp::new(|| {});

    println!("{}", "=".repeat(60).cyan());
    println!("  {} — {}", "PARTS OF SPEECH".bold().bright_white(), "CLI Mode".italic());
    println!("{}", "  (run with --gui for the graphical interface)".dimmed());
    println!("{}", "=".repeat(60).cyan());

    println!("\n{}", "  COMMANDS".underline());
    let commands = [
        ("r", "Record segment", "s", "Stop & Auto-play"),
        ("p", "Play (last/#n)", "pa", "Play full project"),
        ("c", "Confirm take", "x", "Reject take"),
        ("t", "Try again", "q", "List segments"),
        ("u", "Undo", "re", "Redo"),
    ];

    for (cmd1, desc1, cmd2, desc2) in commands {
        println!("    {:>2} {:<18} {:>6} {:<18}", 
            cmd1.bright_green(), desc1.dimmed(), 
            cmd2.bright_green(), desc2.dimmed()
        );
    }
    println!("\n  {}  {} <secs> | {} #n", "TRIM:".dimmed(), "trim start|end".yellow(), "delete".red());
    println!("  {}  {}", "EXIT:".dimmed(), "e (export) | quit".red());
    println!("{}", "-".repeat(60).cyan());

    loop {
        let prompt = {
            let rec = app.recorder.lock().unwrap();
            let count = rec.get_segment_count();
            let playing = rec.playback_state == PlaybackState::Playing;
            
            match rec.state {
                state::AppState::Recording => 
                    format!(" {} {} ", "●".red().blink(), "RECORDING".red().bold()),
                state::AppState::Reviewing => 
                    format!(" {} {} ", "▶".blue(), "REVIEWING".blue().bold()),
                state::AppState::Idle if playing => 
                    format!(" {} {} ({} segs)", "".green(), "PLAYING".green(), count),
                state::AppState::Idle => 
                    format!(" {} {} ({} segs)", "○".dimmed(), "IDLE".dimmed(), count),
            }
        };

        print!("{} {} ", prompt, "❯".bright_cyan());
        use std::io::Write;
        std::io::stdout().flush().unwrap();

        let mut input = String::new();
        std::io::stdin().read_line(&mut input).unwrap();
        let parts: Vec<&str> = input.trim().split_whitespace().collect();
        if parts.is_empty() { continue; }

        match parts[0] {
            "r"  => app.handle_command(Command::StartRecording),
            "s"  => app.handle_command(Command::StopRecording),
            "c"  => app.handle_command(Command::Approve),
            "x"  => app.handle_command(Command::Reject),
            "t"  => app.handle_command(Command::RetryCurrentTake),
            "u"  => app.handle_command(Command::Undo),
            "pa" => app.handle_command(Command::PlayAll),

            // "p" is context-sensitive, during Reviewing it calls play_current_segment()
            // directly (listen again), during Idle it plays a specific or last committed segment
            "p" => {
                // during Reviewing it means "listen again",
                // during Idle it plays a specific or the last committed segment.
                let is_reviewing = matches!(
                    app.recorder.lock().unwrap().state,
                    state::AppState::Reviewing
                );
                if is_reviewing {
                    app.play_current_segment();
                } else if let Some(idx_str) = parts.get(1) {
                    if let Ok(n) = idx_str.parse::<usize>() {
                        app.handle_command(Command::PlaySegment(n - 1));
                    }
                } else { // not reviewing and index isn't passed, just play last segment
                    let count = app.recorder.lock().unwrap().get_segment_count();
                    if count > 0 {
                        app.handle_command(Command::PlaySegment(count - 1));
                    } else {
                        println!("No segments recorded yet.");
                    }
                }
            }
            "retry"  => {
                if let Some(n) = parts.get(1).and_then(|s| s.parse::<usize>().ok()) {
                    app.handle_command(Command::RetrySegment(n - 1));
                }
            }
            "insert" => {
                if let Some(n) = parts.get(1).and_then(|s| s.parse::<usize>().ok()) {
                    app.handle_command(Command::InsertAfter(n - 1));
                }
            }
            "delete" => {
                if let Some(n) = parts.get(1).and_then(|s| s.parse::<usize>().ok()) {
                    app.handle_command(Command::DeleteSegment(n - 1));
                }
            }
            "trim" => {
                if parts.len() < 3 { // requires minimum 3 parts trim + pos + ...
                    println!("Usage: trim start|end [segment_number] seconds");
                    println!("Examples: trim start 0.5  (trim current segment)");
                    println!("          trim end 2 0.3  (trim segment #2)");
                    continue;
                }

                let trim_type = parts[1]; // start or end
                let mut segment_index: Option<usize> = None;
                let seconds_str: &str;

                // could be "trim start 0.5"(current segment) or "trim start 2 0.5"
                if parts.len() == 3 { // current segment
                    seconds_str = parts[2];
                } else if parts.len() == 4 { // idx passed in
                    if let Ok(idx) = parts[2].parse::<usize>() {
                        segment_index = Some(idx - 1); // Convert to 0-based
                        seconds_str = parts[3];
                    } else {
                        println!("Invalid segment number.");
                        continue;
                    }
                } else {
                    println!("Too many arguments.");
                    continue;
                }

                if let Ok(secs) = seconds_str.parse::<f32>() {
                    let cmd = match trim_type { // get specific command
                        "start" => Command::TrimStart(segment_index, secs),
                        "end" => Command::TrimEnd(segment_index, secs),
                        _ => {
                            println!("Unknown trim type. Use 'start' or 'end'.");
                            continue;
                        }
                    };
                    app.handle_command(cmd);
                } else {
                    println!("  Invalid seconds value.");
                }
            }
            "q" => {
                let rec = app.recorder.lock().unwrap();
                if rec.project.segments.is_empty() {
                    println!("  {}", "No segments recorded yet.".italic().dimmed());
                } else {
                    println!("\n  {}", "PROJECT SEGMENTS".underline());
                    for (i, seg) in rec.project.segments.iter().enumerate() {
                        let dur = seg.duration_seconds(rec.project.sample_rate);
                        println!(
                            "  {:>2}. [{}] {:>5.2}s  {}", 
                            (i + 1).to_string().bright_white(),
                            "■".repeat((dur as usize).min(10)).green(), // simple "sparkline"
                            dur,
                            format!("({} samples)", seg.samples.len()).dimmed()
                        );
                    }
                    println!();
                }
            }
            "e" => {
                println!("{} Exporting to output.wav...", "✔".green());
                app.handle_command(Command::Export("output.wav".into())); 
                break; 
            }
            "quit" => break,
            _ => println!("  {} Unknown command.", "×".red()),
        }
    }
}

