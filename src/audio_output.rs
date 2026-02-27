use rodio::{DeviceSinkBuilder, Player, buffer::SamplesBuffer};
use std::num::{NonZeroU16, NonZeroU32}; // positive channel and sample_rate
use crate::state::{Segment, Project};

pub fn play_segment(segment: Segment, sample_rate: u32) {
    let handle = DeviceSinkBuilder::open_default_sink()
        .expect("Failed to open default audio device");
    
    let player = Player::connect_new(handle.mixer()); // connect to audio device
    
    let channels = NonZeroU16::new(1).unwrap();
    let rate = NonZeroU32::new(sample_rate).unwrap();
    let source = SamplesBuffer::new(channels, rate, segment.samples); // copy of audio segment
    
    player.append(source); // add samplesbuffer to player for playback
    player.sleep_until_end(); // blocks thread until playback is finished
}

pub fn play_project(project: &Project) {
    let handle = DeviceSinkBuilder::open_default_sink()
        .expect("Failed to open default audio device");
    
    let player = Player::connect_new(handle.mixer());

    let mut all_samples: Vec<f32> = Vec::new(); // copy of all audio samples
    for seg in &project.segments { // add all project samples to all_samples
        all_samples.extend_from_slice(&seg.samples);
    }

    if all_samples.is_empty() {
        return;
    }

    let channels = NonZeroU16::new(project.channels)
        .expect("Invalid channel count");
    let rate = NonZeroU32::new(project.sample_rate)
        .expect("Invalid sample rate");

    let source = SamplesBuffer::new(channels, rate, all_samples);
    
    player.append(source);
    player.sleep_until_end();
}
