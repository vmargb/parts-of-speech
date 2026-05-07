mod state;
mod audio_input;
mod audio_output;
mod export;
mod gui;
mod themes;

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use cpal::traits::StreamTrait;
use state::{AppState, RecorderState, Command, dispatch_command, PlaybackState, Segment};
use audio_output::{play_segment_async, play_project_async, ProjectSnapshot};
use colored::*;

// ** input **
// Microphone -> audio_input.rs ->(samples only)
// RecorderState.current.samples -> Approve -> Project.segments
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


// -- App settings (non-audio settings, GUI-owned) ---------------------------------

pub struct AppSettings {
    pub trim_preview_secs: f32, // how many seconds to play when previewing a trim edge (start or end)
    pub auto_play_on_stop: bool, // whether to auto-play the current segment immediately after stopping
    pub default_export_dir: Option<String>, // if set, the WAV export file-dialog opens here instead of the project dir
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            trim_preview_secs: 3.0, // previous 3 seconds of s/e
            auto_play_on_stop: true,
            default_export_dir: None,
        }
    }
}


// RecorderApp is the App struct which owns all long-lived resources
// and implements eframe::App
//
// Owned resources:
//   _stream:  must live as long as the app; dropping it silences the mic
//   recorder: Arc<Mutex<RecorderState>> shared with the audio thread
// `_stream` field name starts with `_` so Rust knows the drop is
// intentional (not a bug) and won't emit an unused-variable warning.
pub struct RecorderApp {
    pub recorder:          Arc<Mutex<RecorderState>>,
    pub _stream:           cpal::Stream,
    // GUI state, not visible to audio threads
    pub selected_segment:  Option<usize>,
    pub trim_amount:       f32,
    pub silence_secs:      f32,
    // Shared cancel flag. Set to true to interrupt playback immediately.
    // Reset to false at the start of every new play call.
    pub stop_playback:     Arc<AtomicBool>,
    // Position of the seek bar for the currently-expanded segment (seconds).
    // Reset to 0.0 whenever a different segment is expanded.
    pub seek_offset_secs:  f32,
    // Playback progress tracking (GUI-side, time-based).
    // frame_time is the egui wall-clock time captured once per frame so
    // sub-widgets can read it without calling ctx.input() while holding locks.
    pub frame_time:              f64,
    // Wall time (egui seconds) at which the current play call started.
    pub playback_wall_start:     f64,
    // Offset within the segment (seconds) where the current play call began.
    pub playback_display_offset: f32,
    // Used to detect the Idle→Playing transition each frame.
    pub prev_is_playing:         bool,
    // When the user clicks the seek bar during playback we can't start a new
    // play call immediately (because play_segment_from guards against it while Playing)
    // instead stop playback and store the intent here update() drains it
    // the next frame once the audio thread has actually gone Idle.
    pub pending_seek:            Option<(usize, f32)>,
    pub show_keybindings:  bool,
    pub show_settings:     bool,
    pub theme:             themes::ThemeKind,
    pub palette:           themes::Palette,
    pub settings:          AppSettings,
    // waveform_cache: per-committed-segment peak data (GUI owned, never shared with audio thread)
    pub waveform_cache: HashMap<usize, (usize, Vec<f32>)>,
    // live waveform for the current recording incremental / O(new samples only)
    // live_peaks is a VecDeque of peak values, one per BUCKET_SAMPLES samples
    // At most MAX_LIVE_BUCKETS entries.  Oldest bucket falls/scrolls off
    // the left as recording progresses
    pub live_peaks:         VecDeque<f32>,
    // live_sample_cursor: how many samples from current.samples have already
    // been committed into buckets.  Each update, clone ONLY the slice
    // [live_sample_cursor..], which is always small (~50 ms of audio)
    // which keeps the clone O(new data) instead of O(total recording length)
    pub live_sample_cursor: usize,
    // switch between minimal and advanced mode
    pub minimal_mode: bool,
}

impl RecorderApp {
    pub fn new(on_new_data: impl Fn() + Send + 'static) -> Self {
        // run_gui passes ctx.request_repaint(), while CLI passes || {}
        let recorder = Arc::new(Mutex::new(RecorderState::new(48000, 1)));
        let stream = audio_input::start_input_stream(recorder.clone(), on_new_data);
        stream.play().unwrap();
        let theme = themes::ThemeKind::Dark;
        let palette = themes::palette_for(&theme);
        Self {
            recorder,
            _stream: stream,
            selected_segment:  None,
            trim_amount:       0.10,
            silence_secs:      1.0,
            stop_playback:     Arc::new(AtomicBool::new(false)),
            seek_offset_secs:  0.0,
            frame_time:              0.0,
            playback_wall_start:     0.0,
            playback_display_offset: 0.0,
            prev_is_playing:         false,
            pending_seek:            None,
            show_keybindings:  false,
            show_settings:     false,
            theme,
            palette,
            settings:          AppSettings::default(),
            waveform_cache:    HashMap::new(),
            live_peaks:        VecDeque::new(),
            live_sample_cursor: 0,
            minimal_mode: false,
        }
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
            // reset the stop flag before every new playback call
            self.stop_playback.store(false, Ordering::Relaxed);
            play_segment_async(seg_clone, sample_rate, self.recorder.clone(),
                self.stop_playback.clone(), || {});
        }
    }

    // Play from any offset in a committed segment, optionally capped to max_secs.
    //
    // This is the single unified primitive that backs both the seek bar and the
    // post-trim edge preview.  All the borrow-safety rules are the same as
    pub fn play_segment_from(&self, idx: usize, offset_secs: f32, max_secs: Option<f32>) {
        let rec = self.recorder.lock().unwrap();
        if rec.playback_state == PlaybackState::Playing { return; }
        if let Some(seg) = rec.project.segments.get(idx) {
            let sr = rec.project.sample_rate;
            let offset_samples = (offset_secs * sr as f32) as usize;
            let start = offset_samples.min(seg.samples.len());
            let end = if let Some(max) = max_secs {
                (start + (max * sr as f32) as usize).min(seg.samples.len())
            } else {
                seg.samples.len()
            };
            let preview_samples = seg.samples[start..end].to_vec();
            if preview_samples.is_empty() { return; }
            let preview_seg = Segment { samples: preview_samples, is_silence: false };
            drop(rec);
            self.stop_playback.store(false, Ordering::Relaxed);
            play_segment_async(preview_seg, sr, self.recorder.clone(),
                self.stop_playback.clone(), || {});
        }
    }

    // Play only the first or last `trim_preview_secs` of a committed segment.
    // Called after trimming so the user can immediately hear whether the edit
    // landed correctly, without waiting through the whole clip.
    pub fn play_segment_edge(&self, idx: usize, from_start: bool) {
        // Peek at duration while holding the lock, then drop before playing.
        let dur = {
            let rec = self.recorder.lock().unwrap();
            rec.project.segments.get(idx).map(|s| {
                s.samples.len() as f32 / rec.project.sample_rate as f32
            })
        };
        let preview_secs = self.settings.trim_preview_secs;
        if let Some(dur) = dur {
            let offset = if from_start { 0.0 } else { (dur - preview_secs).max(0.0) };
            self.play_segment_from(idx, offset, Some(preview_secs));
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
                if self.settings.auto_play_on_stop {
                    self.play_current_segment(); // auto-play after stopping
                }
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
                    println!("Wait for playback to finish before retrying.");
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
                    self.stop_playback.store(false, Ordering::Relaxed);
                    play_segment_async(seg_clone, sample_rate, self.recorder.clone(),
                        self.stop_playback.clone(), || {});
                }
            }

            Command::PlayAll => {
                let rec = self.recorder.lock().unwrap();
                if rec.playback_state == PlaybackState::Playing { return; }
                if rec.project.segments.is_empty() { return; }

                let snapshot = ProjectSnapshot::from_project(&rec.project);
                drop(rec);
                self.stop_playback.store(false, Ordering::Relaxed);
                play_project_async(snapshot, self.recorder.clone(),
                    self.stop_playback.clone(), || {});
            }

            // Set the stop flag — the polling loop in the audio thread sees it
            // within ~50ms and drops the player, silencing output immediately.
            // PlaybackState is reset to Idle by the audio thread's on_done path.
            Command::StopPlayback => {
                self.stop_playback.store(true, Ordering::Relaxed);
            }

            Command::Export(custom_path) => {
                let rec = self.recorder.lock().unwrap();
                // Use the provided path, or fallback to output.wav
                let path = custom_path.unwrap_or_else(|| "output.wav".into());
                export::export_wav(&rec.project, &path);
                println!("Exported to {}", path);
            }

            Command::SaveProjectAs(path) => {
                let mut rec = self.recorder.lock().unwrap();
                rec.set_save_path(path.clone()); // Remember where we saved it
                rec.save_to_disk();
                println!("Project saved to {}", path);
            }

            Command::LoadProject(path) => {
                let mut rec = self.recorder.lock().unwrap();
                if let Err(e) = rec.load_from_disk(path.clone()) {
                    eprintln!("Failed to load project: {}", e);
                } else {
                    println!("Project loaded successfully from {}", path);
                }
            }

            // All other commands change state.rs which are delegated to dispatch_command
            other => {
                let mut rec = self.recorder.lock().unwrap();
                dispatch_command(&mut rec, other);
            }

        }
    }
}

// ---------------------------------------------------------------------------
// Waveform helpers
// ---------------------------------------------------------------------------

/// Reduce samples to num_buckets peak-absolute values in [0.0, 1.0]
/// Uses the maximum absolute value in each bucket so quiet passages show as
/// low bars and loud peaks show tall useful for spotting where to trim
pub fn compute_waveform_peaks(samples: &[f32], num_buckets: usize) -> Vec<f32> {
    if samples.is_empty() || num_buckets == 0 {
        return vec![0.0; num_buckets];
    }
    // integer division: how many raw samples map to one display bucket
    // the last bucket may cover fewer samples which is fine for display
    let bucket_size = (samples.len() / num_buckets).max(1);
    (0..num_buckets).map(|i| {
        let start = i * bucket_size;
        let end   = ((i + 1) * bucket_size).min(samples.len());
        if start >= samples.len() { return 0.0_f32; }
        // Peak absolute value in this bucket
        samples[start..end]
            .iter()
            .fold(0.0_f32, |acc, &s| acc.max(s.abs()))
            .clamp(0.0, 1.0)
    }).collect()
}

impl RecorderApp {
    /// Called once per frame from `update()` before any drawing.
    ///
    /// Live waveform incremental, O(new samples only):
    ///   Each call clones only `current.samples[live_sample_cursor..]` — the
    ///   samples that arrived since the last bucket was committed.  That slice
    ///   is bounded to one bucket width (~2 400 samples / 50 ms) regardless of
    ///   how long the take has been running, so cost is constant over time.
    ///   Processed samples are bucketed into `live_peaks` (VecDeque) and
    ///   `live_sample_cursor` advances.  Partial final chunks are left for the
    ///   next frame they never get re-cloned, just extended.
    ///
    /// Segment waveforms invalidated by sample-count change only:
    ///   project.segments is only mutated by GUI-thread commands, so the clone
    ///   here is safe.  Peaks are cached until a trim / delete / approve changes
    ///   the segment, at which point the cache entry is recomputed once.
    ///
    /// Lock discipline: `try_lock` exactly once, snapshot all needed data as
    /// owned values, drop the lock, do all CPU work with no locks held.
    /// If the audio thread holds the lock this frame is skipped silently.
    pub fn update_waveform_caches(&mut self) {
        // One display bucket = this many input samples.
        // At 48 kHz: 2 400 samps = 50 ms.  At 44.1 kHz: ~54 ms.  Fine either way.
        // Keeping this small means we commit a new bucket very frequently, giving
        // a smooth real-time display without ever cloning more than ~19 KB.
        const BUCKET_SAMPLES:   usize = 2_400;
        const MAX_LIVE_BUCKETS: usize = 200; // ~10 s of scrolling history at 50 ms/bucket
        const SEG_BUCKETS:      usize = 200;

        // ── Single non-blocking lock acquisition ────────────────────────────
        let snapshot = match self.recorder.try_lock() {
            Ok(rec) => {
                let is_active = matches!(rec.state,
                    AppState::Recording | AppState::Reviewing);

                // Clone ONLY the unprocessed tail of current.samples.
                // This slice is at most ~BUCKET_SAMPLES long in the steady state
                // because we advance live_sample_cursor after every full bucket.
                let new_tail: Option<Vec<f32>> = if is_active {
                    rec.current.as_ref().and_then(|seg| {
                        let cur_len = seg.samples.len();
                        if cur_len > self.live_sample_cursor {
                            // to_vec() copies only [cursor..end] — bounded clone.
                            Some(seg.samples[self.live_sample_cursor..].to_vec())
                        } else {
                            None
                        }
                    })
                } else {
                    None
                };

                let seg_updates: Vec<(usize, usize, Vec<f32>)> = rec.project.segments
                    .iter()
                    .enumerate()
                    .filter_map(|(idx, seg)| {
                        let cached = self.waveform_cache.get(&idx).map(|(n, _)| *n);
                        let cur    = seg.samples.len();
                        if cached != Some(cur) {
                            Some((idx, cur, seg.samples.clone()))
                        } else { None }
                    })
                    .collect();

                let seg_count = rec.project.segments.len();
                Some((is_active, new_tail, seg_updates, seg_count))
            }
            Err(_) => None,
        };
        // Lock is dropped here all work below operates on owned data

        let Some((is_active, new_tail, seg_updates, seg_count)) = snapshot
        else { return; };

        // -- Live waveform -------------------------------------------------
        if !is_active {
            // State left recording/reviewing — reset for next take.
            if !self.live_peaks.is_empty() {
                self.live_peaks.clear();
                self.live_sample_cursor = 0;
            }
        } else if let Some(tail) = new_tail {
            // Bucket the new tail samples.  Only full BUCKET_SAMPLES chunks are
            // committed; a partial final chunk waits until next frame.
            // This means live_sample_cursor always points at a bucket boundary,
            // so the clone next frame picks up exactly where we left off.
            for chunk in tail.chunks(BUCKET_SAMPLES) {
                if chunk.len() < BUCKET_SAMPLES {
                    break; // partial — leave for next frame
                }
                let peak = chunk.iter()
                    .fold(0.0_f32, |acc, &s| acc.max(s.abs()))
                    .clamp(0.0, 1.0);
                self.live_peaks.push_back(peak);
                if self.live_peaks.len() > MAX_LIVE_BUCKETS {
                    self.live_peaks.pop_front(); // scroll: oldest falls off left
                }
                self.live_sample_cursor += BUCKET_SAMPLES;
            }
        }

        // Segment caches
        for (idx, sample_count, samples) in seg_updates {
            let peaks = compute_waveform_peaks(&samples, SEG_BUCKETS);
            self.waveform_cache.insert(idx, (sample_count, peaks));
        }

        // Evict entries whose index no longer exists (segment was deleted / project cleared).
        self.waveform_cache.retain(|&k, _| k < seg_count);
    }
}

fn run_gui() {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([740.0, 600.0])
            .with_min_inner_size([480.0, 400.0])
            .with_title("Parts of Speech"),
        ..Default::default()
    };

    eframe::run_native(
        "Parts of Speech",
        options,
        Box::new(|cc| {
            let ctx = cc.egui_ctx.clone();
            Ok(Box::new(RecorderApp::new(move || ctx.request_repaint())))
        }),
    ).unwrap();
}

fn run_cli() {
    let app = RecorderApp::new(|| {});
    let mut clear = true;

    loop {
        // Clear the screen and move cursor to home position
        if clear {
            print!("\x1B[2J\x1B[H");
            println!("{}", "=".repeat(60).cyan());
            println!("  {} : {}", "PARTS OF SPEECH".bold().bright_white(), "CLI Mode".italic());
            println!("{}", "  (run with --gui for the graphical interface)".dimmed());
            println!("{}", "=".repeat(60).cyan());
            println!("\n{}", "  COMMANDS".underline());
            let commands = [
                ("r",  "Record segment",   "s",  "Stop & Auto-play"),
                ("p",  "Play (last/#n)",   "pa", "Play full project"),
                ("c",  "Confirm take",     "x",  "Reject take"),
                ("t",  "Try again",        "q",  "List segments"),
                ("u",  "Undo",             "z",  "Redo"),
            ];

            for (cmd1, desc1, cmd2, desc2) in commands {
                println!("    {:>2} {:<18} {:>6} {:<18}", 
                    cmd1.bright_green(), desc1.dimmed(),
                    cmd2.bright_green(), desc2.dimmed()
                );
            }
            println!("\n  {}  {} <secs> | {} #n", "TRIM:".dimmed(), "trim start|end".yellow(), "delete".red());
            println!("  {}  {} #n [secs]  |  {} #n [secs]", "SILENCE:".dimmed(), "silence".yellow(), "expand".yellow());
            println!("  {}  {}", "EXIT:".dimmed(), "e (export) | quit".red());
            println!("{}", "-".repeat(60).cyan());
        }
        clear = true;

        let prompt = {
            let rec = app.recorder.lock().unwrap();
            let count = rec.get_segment_count();
            let total_time = rec.total_duration();
            let playing = rec.playback_state == PlaybackState::Playing;
            
            match rec.state {
                state::AppState::Recording => 
                    format!(" {} {} ", "●".red().blink(), "RECORDING".red().bold()),
                state::AppState::Reviewing => 
                    format!(" {} {} ", "▶".blue(), "REVIEWING".blue().bold()),
                state::AppState::Idle if playing => 
                    format!(" {} {} ({} segs)", "".green(), "PLAYING".green(), count),
                state::AppState::Idle => 
                    format!(" {} {} ({} segs, {})", "○".dimmed(), "IDLE".dimmed(), count, total_time),
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
            // "stop" in CLI stops playback (not "s" which stops recording)
            "stop" => app.handle_command(Command::StopPlayback),
            "c"  => app.handle_command(Command::Approve),
            "x"  => app.handle_command(Command::Reject),
            "t"  => app.handle_command(Command::RetryCurrentTake),
            "u"  => app.handle_command(Command::Undo),
            "z"  => app.handle_command(Command::Redo),
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
            // insert a silence segment after segment #n (1-based).
            // silence <n> [seconds]
            "silence" => {
                if let Some(n) = parts.get(1).and_then(|s| s.parse::<usize>().ok()) {
                    let secs = parts.get(2)
                        .and_then(|s| s.parse::<f32>().ok())
                        .unwrap_or(1.0)
                        .max(0.01);
                    app.handle_command(Command::InsertSilenceAfter(n - 1, secs));
                    println!("Inserted {:.2}s silence after segment {}.", secs, n);
                } else {
                    println!("Usage: silence <segment_number> [seconds]");
                    println!("Example: silence 2       (1s of silence after segment 2)");
                    println!("         silence 2 0.5   (0.5s of silence after segment 2)");
                }
            }
            // Expand an existing silence segment by adding more silence.
            // expand <n> [seconds]
            "expand" => {
                if let Some(n) = parts.get(1).and_then(|s| s.parse::<usize>().ok()) {
                    let secs = parts.get(2)
                        .and_then(|s| s.parse::<f32>().ok())
                        .unwrap_or(0.5)
                        .max(0.01);
                    app.handle_command(Command::ExpandSilence(n - 1, secs));
                    println!("Expanded segment {} by {:.2}s.", n, secs);
                } else {
                    println!("Usage: expand <segment_number> [seconds]");
                    println!("Example: expand 3       (add 0.5s to silence segment 3)");
                    println!("         expand 3 1.5   (add 1.5s to silence segment 3)");
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
                    println!("Invalid seconds value.");
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
                        if seg.is_silence {
                            println!(
                                "  {:>2}. [{}] {:>5.2}s  {}",
                                (i + 1).to_string().bright_white(),
                                "~".repeat((dur as usize).min(10)).cyan(),
                                dur,
                                "(silence)".dimmed()
                            );
                        } else {
                            println!(
                                "  {:>2}. [{}] {:>5.2}s  {}", 
                                (i + 1).to_string().bright_white(),
                                "■".repeat((dur as usize).min(10)).green(),
                                dur,
                                format!("({} samples)", seg.samples.len()).dimmed()
                            );
                        }
                    }
                    println!();
                }
                clear = false;
            }
            "e" => {
                println!("{} Exporting to output.wav...", "✔".green());
                app.handle_command(Command::Export(Some("output.wav".into()))); 
                break; 
            }
            "quit" => { print!("\x1B[2J\x1B[H"); break; }
            _ => println!("  {} Unknown command.", "×".red()),
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.contains(&"--cli".to_string()) {
        run_cli();
    } else {
        run_gui();
    }
}
