mod state;
mod audio_input;
mod audio_output;
mod export;

use std::sync::{Arc, Mutex};

use state::RecorderState;

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
    let recorder = Arc::new(Mutex::new(
        RecorderState::new(44100, 1),
    ));

    let stream = audio_input::start_input_stream(recorder.clone());
    stream.play().unwrap();

    // temporary CLI control loop for now (replace with GUI later)
    loop {
        println!("r=start, s=stop, c=confirm, x=reject, p=play last, e=export");

        let mut input = String::new();
        std::io::stdin().read_line(&mut input).unwrap();

        let mut recorder = recorder.lock().unwrap();

        match input.trim() {
            "r" => recorder.start_recording(), // record
            "s" => recorder.stop_recording(), // stop
            "c" => recorder.approve(), // confirm
            "x" => recorder.reject(), // reject
            "p" => { // play
                if let Some(seg) = recorder.project.segments.last() {
                    drop(recorder); // unlock before playback
                    audio_output::play_segment(seg, 44100);
                }
            }
            "e" => { // export
                export::export_wav(&recorder.project, "output.wav");
                break;
            }
            _ => {}
        }
    }
}
