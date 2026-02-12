use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::{Arc, Mutex};

use crate::state::{AppState, RecorderState};

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
            move |data: &[f32], _| {
                let mut recorder = recorder.lock().unwrap();

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
