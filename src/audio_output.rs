use rodio::{DeviceSinkBuilder, Player, buffer::SamplesBuffer};
use std::num::{NonZeroU16, NonZeroU32}; // positive channel and sample_rate
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
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

// sentinel stored in `current_segment` when no project playback is running
pub const NO_SEGMENT: usize = usize::MAX;

// plays the snapshot, which may start part-way through the project
// ProjectSnapshot::from_project) as a queue of one source per segment
pub fn play_project_async(
    project_snapshot: ProjectSnapshot, // copy of the project from `first_index` onward
    recorder: Arc<Mutex<crate::state::RecorderState>>,
    stop_flag: Arc<AtomicBool>,
    current_segment: Arc<AtomicUsize>,
    on_done: impl Fn() + Send + 'static,
) {
    {
        let mut rec = recorder.lock().unwrap();
        rec.playback_state = PlaybackState::Playing;
    }

    std::thread::spawn(move || {
        // build the queue first. Empty segments are skipped, so remember which project index each
        // queued source belongs to
        let rate = NonZeroU32::new(project_snapshot.sample_rate)
            .expect("Invalid sample rate");
        let channels = NonZeroU16::new(1).unwrap(); // force mono like play_segment_async

        let mut order: Vec<usize> = Vec::new(); // order[k] = project index of k-th source
        let mut sources: Vec<SamplesBuffer> = Vec::new();
        for (offset, samples) in project_snapshot.segments.into_iter().enumerate() {
            if samples.is_empty() { continue; }
            order.push(project_snapshot.first_index + offset);
            sources.push(SamplesBuffer::new(channels, rate, samples));
        }

        if sources.is_empty() {
            if let Ok(mut rec) = recorder.lock() {
                rec.playback_state = PlaybackState::Idle;
            }
            current_segment.store(NO_SEGMENT, Ordering::Relaxed);
            on_done();
            return;
        }

        let mut handle = DeviceSinkBuilder::open_default_sink()
            .expect("Failed to open default audio device");
        handle.log_on_drop(false);

        let player = Player::connect_new(handle.mixer());

        let total = sources.len();
        current_segment.store(order[0], Ordering::Relaxed);
        for source in sources {
            player.append(source);
        }

        // poll until audio ends naturally or stop_flag fires
        while !player.empty() && !stop_flag.load(Ordering::Relaxed) {
            let played = total.saturating_sub(player.len()); // finished sources
            if let Some(&seg_idx) = order.get(played) {
                current_segment.store(seg_idx, Ordering::Relaxed);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        drop(player);
        current_segment.store(NO_SEGMENT, Ordering::Relaxed);

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
    // project index of segments[0]; non-zero when playing "from here"
    pub first_index: usize,
    // channels always mono so not needed
}

impl ProjectSnapshot {
    // clones only the segments from `first_index` to the end, "play from here"
    // doesn't copy audio that will never be played
    pub fn from_project(project: &Project, first_index: usize) -> Self {
        let first_index = first_index.min(project.segments.len());
        Self {
            segments: project.segments[first_index..]
                .iter()
                .map(|s| s.samples.clone())
                .collect(),
            sample_rate: project.sample_rate,
            first_index,
        }
    }
}
