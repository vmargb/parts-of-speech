use rodio::{OutputStream, Sink, Source};
use std::time::Duration;

use crate::state::Segment;

pub fn play_segment(segment: &Segment, sample_rate: u32) {
    let (_stream, handle) = OutputStream::try_default().unwrap();
    let sink = Sink::try_new(&handle).unwrap();

    let source = rodio::buffer::SamplesBuffer::new(
        1,
        sample_rate,
        segment.samples.clone(),
    );

    sink.append(source);
    sink.sleep_until_end();
}
