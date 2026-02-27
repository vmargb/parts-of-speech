use cpal::traits::{DeviceTrait, HostTrait};
use std::sync::{Arc, Mutex};

use crate::state::{AppState, RecorderState};

// start_input_stream is a background thread
// thats constantly listening to the mic
pub fn start_input_stream(
    recorder: Arc<Mutex<RecorderState>>,
) -> cpal::Stream {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .expect("No input device");

    let config = device.default_input_config().unwrap();

    let stream = device
        .build_input_stream(
            &config.into(),
            // callback function that runs every time a
            // buffer of sound is captured by the mic
            move |data: &[f32], _| {
                let mut recorder = recorder.lock().unwrap(); // safely access data

                // appends the sound data to current.samples
                if let AppState::Recording = recorder.state {
                    if let Some(seg) = recorder.current.as_mut() {
                        seg.samples.extend_from_slice(data);
                    }
                }
            },
            |err| eprintln!("input error: {:?}", err),
            None,
        )
        .unwrap();

    stream
}
// this audio thread runs separately from the main loop
// but needs a safe way to share the RecorderState (Arc<Mutex<RecorderState>>)
