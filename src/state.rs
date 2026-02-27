// This module is the data model that holds audio
// segments linearly. Nothing outside of this module
// is allowed to mutate segments directly

// ===== Data =====

#[derive(Clone)]
pub struct Segment { // a single recording take
    // the actual audio numbers
    pub samples: Vec<f32>, // raw audio data (32-bit float samples)
}
// a segment is one recorded chunk
// #[derive(clone)] lets you duplicate a segment
// for recording replacements (retry)

#[allow(unused)]
impl Segment {
    // 1 second of pub samples = 44100 indexes (sample_rate)
    pub fn duration_seconds(&self, sample_rate: u32) -> f32 {
        self.samples.len() as f32 / sample_rate as f32
    }
}

pub struct Project {
    pub segments: Vec<Segment>, // ALL chunks in order
    pub sample_rate: u32, // 44100 or 48000 Hz
    pub channels: u16,    // 1: mono, 2: stereo
    pub editing_index: Option<usize>, // which segment we're editing (for retry/insert)
}
// persistent timeline of all segments (that were approved)
// added together

// ===== State =====

pub enum AppState {
    Idle,
    Recording,
    Reviewing, // keep or retry?
}

pub struct RecorderState {
    pub state: AppState,
    pub current: Option<Segment>, // current chunk being recorded/reviewed
    pub project: Project, // all chunks
}
// full picture of the state is held inside RecorderState


// holds the the current segment being recorded, the state
// and the project in which it will add the approved recording to
impl RecorderState { // master struct
    pub fn new(sample_rate: u32, channels: u16) -> Self {
        Self {
            state: AppState::Idle,
            current: None, // current recording segment
            project: Project {
                segments: Vec::new(),
                sample_rate,
                channels,
                editing_index: None,
            },
        }
    }

    pub fn total_duration(&self) -> f32 {
        self.project.segments
            .iter()
            .map(|seg| seg.duration_seconds(self.project.sample_rate))
            .sum()
    }

    // create an empty segment and start recording
    pub fn start_recording(&mut self) {
        self.state = AppState::Recording;
        self.current = Some(Segment {
            samples: Vec::new(),
        });
        self.project.editing_index = None; // None: segment at end default
    }

    pub fn stop_recording(&mut self) {
        self.state = AppState::Reviewing; // review recording
    }

    // appends the approved segment into project.segments
    pub fn approve(&mut self) {
        if let Some(seg) = self.current.take() { // current segment exists
            if let Some(idx) = self.project.editing_index.take() { // idx is provided
                if idx < self.project.segments.len() { // in bound
                    self.project.segments[idx] = seg; // replace
                }
            } else { // idx not provided, default to end of project
                self.project.segments.push(seg);
            }
        }
        self.state = AppState::Idle;
    }
    // retry can provide an idx number, which is held in struct
    // so approve accounts for both cases

    pub fn reject(&mut self) {
        self.current = None; // delete current segment
        self.project.editing_index = None;
        self.state = AppState::Idle;
    }

    // rerecord previous segments that you approved
    #[allow(unused)]
    pub fn replace_segment(&mut self, index: usize, new_segment: Segment) {
        self.project.segments[index] = new_segment;
    }

    pub fn insert_segment(&mut self, after_index: usize) -> bool {
        if after_index >= self.project.segments.len() {
            return false;
        }
        self.current = Some(Segment { samples: Vec::new() });
        self.project.editing_index = Some(after_index + 1); // insert after
        self.state = AppState::Recording;
        true
    }

    pub fn get_segment(&self, index: usize) -> Option<&Segment> {
        if index == 0 || index > self.project.segments.len() {
            return None;
        }
        self.project.segments.get(index - 1) // 1-based for user
    }

    pub fn get_segment_count(&self) -> usize {
        self.project.segments.len()
    }

    // optionally add empty segments in between recordings
    // silence(0.5, sample_rate) would add a 0.5s silence
    fn silence(seconds: f32, sample_rate: u32) -> Segment {
        let count = (seconds * sample_rate as f32) as usize;
        Segment {
            samples: vec![0.0; count],
        }
    }
}
// write logic for RecorderState without audio
// unit test the entire workflow without needing audio
