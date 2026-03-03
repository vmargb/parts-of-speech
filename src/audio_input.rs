use cpal::traits::{DeviceTrait, HostTrait};
use std::sync::{Arc, Mutex};

use crate::state::{AppState, RecorderState};

// start_input_stream is a background thread
// that is always listening to the mic
// but only saves audio when AppState::Recording.

pub fn start_input_stream(
    recorder: Arc<Mutex<RecorderState>>,
    on_new_data: impl Fn() + Send + 'static, // callback function to ctx.request_repaint
) -> cpal::Stream {
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
            // try_lock keeps the continuous audio callback non-blocking
            if let Ok(mut rec) = recorder.try_lock() {
                if let AppState::Recording = rec.state {
                    if let Some(seg) = rec.current.as_mut() { // samples inserted here
                        if hardware_channels == 1 { // mono, just copy
                            seg.samples.extend_from_slice(data);
                        } else {
                            // Stereo (or more), down-mix to Mono
                            // .chunks_exact(2) gives [[L, R], [L, R], ...] so L + R / 2
                            let mono_data = data //.into() converts u16 into usize
                                .chunks_exact(hardware_channels.into())
                                .map(|frame| frame.iter().sum::<f32>() / hardware_channels as f32);
                            seg.samples.extend(mono_data);
                        }
                        on_new_data(); // samples have arrived so it can repaint
                    }
                }
            }
        },
        |err| eprintln!("input error: {:?}", err),
        None,
    ).unwrap();

    stream
}
