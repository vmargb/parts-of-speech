mod state;
mod audio_input;
mod audio_output;
mod export;

use std::sync::{Arc, Mutex};
use cpal::traits::StreamTrait;
use state::{RecorderState, Command, dispatch_command, PlaybackState};
use audio_output::{play_segment_async, play_project_async, ProjectSnapshot};

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
    pub fn new() -> Self {
        let recorder = Arc::new(Mutex::new(RecorderState::new(44100, 1)));
        // The `on_new_data` callback will become `ctx.request_repaint` in egui.
        // For now it's a no-op so the wiring compiles without eframe.
        let on_new_data = || {};
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
    let app = RecorderApp::new();

    println!("Commands: r  s  c  x  p <n>  pa  retry <n>  insert <n>  delete <n>  e  q  quit");

    loop {
        let status = {
            let rec = app.recorder.lock().unwrap();
            let count = rec.get_segment_count();
            let playing = rec.playback_state == PlaybackState::Playing;
            match rec.state {
                state::AppState::Idle if playing =>
                    format!("Playing... ({} segments)", count),
                state::AppState::Idle =>
                    format!("Idle ({} segments)", count),
                state::AppState::Recording =>
                    "Recording...".to_string(),
                state::AppState::Reviewing =>
                    "Reviewing / c=confirm x=reject t=try-again p=listen again".to_string(),
            }
        };

        print!("{} > ", status);
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
            "pa" => app.handle_command(Command::PlayAll),

            // "p" is context-sensitive, during Reviewing it calls play_current_segment()
            // directly (listen again), during Idle it plays a specific or last committed segment
            "p" => {
                // Context-sensitive: during Reviewing it means "listen again",
                // during Idle it plays a specific or the last committed segment.
                let is_reviewing = matches!(
                    app.recorder.lock().unwrap().state,
                    state::AppState::Reviewing
                );
                if is_reviewing { // listen again during reviewing
                    app.play_current_segment();
                } else if let Some(idx_str) = parts.get(1) { // idx is passed e.g. p 2
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

            "q" => {
                let rec = app.recorder.lock().unwrap();
                if rec.project.segments.is_empty() {
                    println!("  No segments yet.");
                } else {
                    for (i, seg) in rec.project.segments.iter().enumerate() {
                        println!("    #{} — {} samples  ({:.2}s)",
                            i + 1, seg.samples.len(),
                            seg.duration_seconds(rec.project.sample_rate));
                    }
                }
            }
            "e" => {
                app.handle_command(Command::Export("output.wav".into()));
                break;
            }
            "quit" => break,
            _ => println!("  Unknown command."),
        }
    }
}
