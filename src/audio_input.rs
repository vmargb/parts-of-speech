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
    let hardware_sample_rate = config.sample_rate(); // cpal::SampleRate
    let hardware_channels = config.channels(); // u16

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
            // Determine whether new samples were written with the mutex held,
            // then call on_new_data() AFTER releasing it to fix deadlock
            //
            // on_new_data() calls ctx.request_repaint() which
            // acquires egui's internal lock. The GUI thread can be inside
            // ctx.input() (egui lock held) while waiting for the recorder
            // mutex. If calling on_new_data() while still holding the recorder
            // mutex we get a lock-order inversion and the app freezes
            // releasing the mutex first breaks the cycle.
            let should_repaint = if let Ok(mut rec) = recorder.try_lock() {
                if let AppState::Recording = rec.state {
                    if let Some(seg) = rec.current.as_mut() {
                        if hardware_channels == 1 { // mono, just copy
                            seg.samples.extend_from_slice(data);
                        } else {
                            // stereo (or more), down-mix to Mono
                            // .chunks_exact(2) gives [[L, R], [L, R], ...] so L + R / 2
                            let mono_data = data //.into() converts u16 into usize
                                .chunks_exact(hardware_channels as usize)
                                .map(|frame| frame.iter().sum::<f32>() / hardware_channels as f32);
                            seg.samples.extend(mono_data);
                        }
                        true // samples written, request repaint
                    } else { false }
                } else { false }
            } else { false }; // try_lock failed, skip this callback

            if should_repaint { // prevent deadlock
                on_new_data(); // called with no locks held safe
            }
        },
        |err| eprintln!("input error: {:?}", err),
        None,
    ).unwrap();

    stream
}
