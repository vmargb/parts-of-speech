use rodio::{DeviceSinkBuilder, Player, buffer::SamplesBuffer};
use std::num::{NonZeroU16, NonZeroU32}; // positive channel and sample_rate
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use crate::state::{Segment, Project, PlaybackState};

// For output, spawn a thread to do the playback. When it finishes
// (either naturally or via stop_flag), it sets playback_state back
// to Idle so the UI can react.
//
// stop_flag is an Arc<AtomicBool> shared with RecorderApp.
// Setting it to true causes the polling loop below to exit early,
// which drops `player` and immediately silences the device.
// RecorderApp resets it to false before every new playback call.

pub fn play_segment_async(
    segment: Segment,
    sample_rate: u32,
    recorder: Arc<Mutex<crate::state::RecorderState>>,
    stop_flag: Arc<AtomicBool>,
    on_done: impl Fn() + Send + 'static,
) {
    // set as playing before spawning to disable input
    {
        let mut rec = recorder.lock().unwrap();
        rec.playback_state = PlaybackState::Playing;
    }

    std::thread::spawn(move || {
        let mut handle = DeviceSinkBuilder::open_default_sink()
            .expect("Failed to open default audio device");
        handle.log_on_drop(false);

        let player = Player::connect_new(handle.mixer()); // connect to audio device

        let channels = NonZeroU16::new(1).unwrap(); // segments are always mono now after downmix
        let rate = NonZeroU32::new(sample_rate).unwrap();
        let source = SamplesBuffer::new(channels, rate, segment.samples); // copy of audio segment

        player.append(source);

        // Poll until audio ends naturally or stop_flag fires.
        // Dropping `player` when the flag fires cuts audio immediately.
        while !player.empty() && !stop_flag.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(50));
        }
        drop(player); // silence device immediately on early stop

        if let Ok(mut rec) = recorder.lock() {
            rec.playback_state = PlaybackState::Idle;
        }
        on_done(); // callback, update UI
    });
}

pub fn play_project_async(
    project_snapshot: ProjectSnapshot, // copy of whole project
    recorder: Arc<Mutex<crate::state::RecorderState>>,
    stop_flag: Arc<AtomicBool>,
    on_done: impl Fn() + Send + 'static,
) {
    {
        let mut rec = recorder.lock().unwrap();
        rec.playback_state = PlaybackState::Playing;
    }


    std::thread::spawn(move || {
        let mut handle = DeviceSinkBuilder::open_default_sink()
            .expect("Failed to open default audio device");
        handle.log_on_drop(false);

        let player = Player::connect_new(handle.mixer());

        let mut all_samples: Vec<f32> = Vec::new(); // copy of all audio samples
        for samples in project_snapshot.segments { // add all project samples to all_samples
            all_samples.extend(samples);
        }

        if all_samples.is_empty() {
            if let Ok(mut rec) = recorder.lock() {
                rec.playback_state = PlaybackState::Idle;
            }
            on_done();
            return;
        }

        let channels = NonZeroU16::new(1).unwrap(); // force mono like play_segment_async
        let rate = NonZeroU32::new(project_snapshot.sample_rate)
            .expect("Invalid sample rate");

        let source = SamplesBuffer::new(channels, rate, all_samples);
        player.append(source);

        while !player.empty() && !stop_flag.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(50));
        }
        drop(player);

        if let Ok(mut rec) = recorder.lock() {
            rec.playback_state = PlaybackState::Idle;
        }
        on_done();

    });
}

// *** plain-data snapshot of the project
// the problem is we can't send &Project across threads (because its behind
// a mutex and non-Send types), so instead clone the data before spawning
// since it's just Vec<Vec<f32>> + two integers, this is feasible
pub struct ProjectSnapshot {
    pub segments: Vec<Vec<f32>>,
    pub sample_rate: u32,
    // channels always mono so not needed
}

impl ProjectSnapshot {
    pub fn from_project(project: &Project) -> Self {
        Self {
            segments: project.segments.iter().map(|s| s.samples.clone()).collect(),
            sample_rate: project.sample_rate,
        }
    }
}
