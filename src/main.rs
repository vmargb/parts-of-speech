mod state;
mod audio_input;
mod audio_output;
mod export;

use std::sync::{Arc, Mutex};
use state::RecorderState;
use cpal::traits::StreamTrait;

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

fn main() {
    let recorder_state = Arc::new(Mutex::new(
        RecorderState::new(44100, 1),
    ));
    
    let stream = audio_input::start_input_stream(recorder_state.clone());
    stream.play().unwrap(); // StreamTrait

    println!("️Audio Recorder - Non-linear Editing Mode");
    println!("Commands:");
    println!("  r           → Record new segment (appends to end)");
    println!("  s           → Stop recording current segment");
    println!("  p           → Play last recorded segment");
    println!("  p <n>       → Play segment #n (e.g., p 5)");
    println!("  pa          → Play ALL segments (full project)");
    println!("  retry <n>   → Re-record segment #n");
    println!("  insert <n>  → Insert new segment AFTER #n");
    println!("  c           → Confirm current segment");
    println!("  x           → Reject current segment");
    println!("  e           → Export and exit");
    println!("  q           → Show segment list");
    println!();

    loop {
       let recorder = recorder_state.lock().unwrap(); // mutexguard
        let count = recorder.get_segment_count();
        let status = match recorder.state {
            state::AppState::Idle => format!("Idle ({} segments)", count),
            state::AppState::Recording => "Recording...".to_string(),
            state::AppState::Reviewing => "Reviewing (c=confirm, x=reject)".to_string(),
        };
        print!("{} > ", status);
        drop(recorder); // Unlock before reading input

        let mut input = String::new();
        std::io::stdin().read_line(&mut input).unwrap();
        let parts: Vec<&str> = input.trim().split_whitespace().collect();

        if parts.is_empty() {
            continue;
        }

        let mut recorder = recorder_state.lock().unwrap();

        match parts[0] {
            "r" => {
                recorder.start_recording();
                println!("Recording new segment...");
            }
            "s" => {
                recorder.stop_recording();
                println!("Stopped. Press 'c' to confirm or 'x' to reject.");
            }
            "c" => {
                recorder.approve();
                println!("Segment confirmed!");
            }
            "x" => {
                recorder.reject();
                println!("Segment rejected.");
            }
            "p" => {
                drop(recorder); // Release the primary loop lock so playback doesn't block input

                if parts.len() > 1 {
                    // Case: p <n>
                    if let Ok(idx) = parts[1].parse::<usize>() {
                        let rec = recorder_state.lock().unwrap(); // Use recorder_state, not recorder
                        if idx > 0 && idx <= rec.get_segment_count() {
                            if let Some(seg) = rec.get_segment(idx - 1) { // Assuming 1-based input
                                println!("Playing segment {}...", idx);
                                audio_output::play_segment(seg.clone(), 44100);
                            }
                        } else {
                            println!("Segment {} not found", idx);
                        }
                    }
                } else {
                    // Case: p (last segment)
                    let rec = recorder_state.lock().unwrap(); // Use recorder_state
                    if let Some(seg) = rec.project.segments.last() {
                        println!("Playing last segment...");
                        audio_output::play_segment(seg.clone(), 44100);
                    } else {
                        println!("No segments recorded yet");
                    }
                }
            }
            "pa" => {
                drop(recorder); // Release the primary loop lock
                let rec = recorder_state.lock().unwrap(); // Use recorder_state
                if rec.project.segments.is_empty() {
                    println!("No segments to play");
                } else {
                    println!("Playing full project ({} segments)...", rec.get_segment_count());
                    audio_output::play_project(&rec.project);
                }
            }
            "retry" => {
                if let Some(idx_str) = parts.get(1) {
                    if let Ok(idx) = idx_str.parse::<usize>() {
                        if idx > 0 && recorder.retry_segment(idx - 1) { // convert to 0-based
                            println!("  → Re-recording segment {}...", idx);
                        } else {
                            println!("  ✗ Invalid segment number.");
                        }
                    }
                }
            }
            "insert" => {
                if parts.len() > 1 { // at least 1 segment
                    if let Ok(idx) = parts[1].parse::<usize>() {
                        if idx == 0 || idx > recorder.get_segment_count() {
                            println!("Invalid segment number (1-{})", recorder.get_segment_count());
                        } else {
                            recorder.insert_segment(idx - 1); // 1-based index
                            println!("Inserting new segment after #{}...", idx);
                        }
                    }
                } else {
                    println!("Usage: insert <segment_number>");
                }
            }
            "delete" => {
                if let Some(idx_str) = parts.get(1) {
                    if let Ok(idx) = idx_str.parse::<usize>() {
                        if idx > 0 && recorder.delete_segment(idx - 1) { // convert to 0-based
                            println!("  🗑️ Segment {} deleted.", idx);
                        } else {
                            println!("  ✗ Invalid segment number.");
                        }
                    }
                }
            }
            "q" => {
                println!("  📋 Segments:");
                for (i, _) in recorder.project.segments.iter().enumerate() {
                    println!("     #{} ({} samples)", i + 1, recorder.project.segments[i].samples.len());
                }
            }
            "e" => {
                export::export_wav(&recorder.project, "output.wav");
                println!("Exported to output.wav");
                break;
            }
            _ => {
                println!("Unknown command. Type 'h' for help.");
            }
        }
    }
}
