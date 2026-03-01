use cpal::traits::{DeviceTrait, HostTrait};
use std::sync::{Arc, Mutex};

use crate::state::{AppState, RecorderState};

// start_input_stream is a background thread
// thats constantly listening to the mic
// but needs a safe way to share the RecorderState (Arc<Mutex<RecorderState>>)
pub fn start_input_stream(recorder: Arc<Mutex<RecorderState>>) -> cpal::Stream {
    let host = cpal::default_host();
    let device = host.default_input_device().expect("No input device");
    
    // get hardware config
    let config = device.default_input_config().expect("Failed to get default input config");
    let hardware_sample_rate = config.sample_rate();
    let hardware_channels = config.channels();

    // sync RecorderState to hardware settings to avoid mismatch
    // e.g. mic set to 48000Hz in OS settings, but RecorderState 44100
    {
        let mut rec = recorder.lock().unwrap();
        rec.project.sample_rate = hardware_sample_rate;
        rec.project.channels = hardware_channels;
        println!("Hardware: {}Hz, {} channel(s)", hardware_sample_rate, hardware_channels);
    }

    let stream = device.build_input_stream(
        &config.into(),
        move |data: &[f32], _| {
            // try_lock to avoid blocking the audio thread if main is busy
            if let Ok(mut recorder) = recorder.try_lock() {
                if let AppState::Recording = recorder.state {
                    if let Some(seg) = recorder.current.as_mut() {
                        seg.samples.extend_from_slice(data);
                    }
                }
            }
        },
        |err| eprintln!("input error: {:?}", err),
        None,
    ).unwrap();

    stream
}
