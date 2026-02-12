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
            "r" => recorder.start_recording(),
            "s" => recorder.stop_recording(),
            "c" => recorder.approve(),
            "x" => recorder.reject(),
            "p" => {
                if let Some(seg) = recorder.project.segments.last() {
                    drop(recorder); // unlock before playback
                    audio_output::play_segment(seg, 44100);
                }
            }
            "e" => {
                export::export_wav(&recorder.project, "output.wav");
                break;
            }
            _ => {}
        }
    }
}
