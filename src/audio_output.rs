use rodio::{DeviceSinkBuilder, Player, buffer::SamplesBuffer};
use std::num::{NonZeroU16, NonZeroU32}; // positive channel and sample_rate
use std::sync::{Arc, Mutex};
use crate::state::{Segment, Project, PlaybackState};

// For output, spawn a thread to do the playback. When it finishes,
// it sets playback_state back to Idle so the UI can react.
//
// The recorder Arc is passed in so the thread can:
//   1. set PlaybackState::Playing before starting
//   2. set PlaybackState::Idle when done
//   3. call ctx.request_repaint() so egui redraws (passed as callback)
//
pub fn play_segment_async(
    segment: Segment,
    sample_rate: u32,
    recorder: Arc<Mutex<crate::state::RecorderState>>,
    on_done: impl fn() + Send + 'static // callback after playback finished
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

        player.append(source); // add samplesbuffer to player for playback
        player.sleep_until_end(); // blocking until playback finished (safe since new thread)

        //playback is finished at this point
        if let Ok(mut rec) = recorder.lock() {
            rec.playback_state = PlaybackState::Idle; 
        }
        on_done(); // callback, update UI
    });
}

pub fn play_project_async(
    project_snapshot: ProjectSnapshot, // copy of whole project
    recorder: Arc<Mutex<crate::state::RecorderState>>,
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

        let channels = NonZeroU16::new(project.channels)
            .expect("Invalid channel count");
        let rate = NonZeroU32::new(project.sample_rate)
            .expect("Invalid sample rate");

        let source = SamplesBuffer::new(channels, rate, all_samples);
        player.append(source);
        player.sleep_until_end();

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
    pub channels: u16,
}

impl ProjectSnapshot {
    pub fn from_project(project: &Project) -> Self {
        Self {
            segments: project.segments.iter().map(|s| s.samples.clone()).collect(),
            sample_rate: project.sample_rate,
            channels: project.channels,
        }
    }
}

