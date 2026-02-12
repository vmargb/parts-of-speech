// This module is the data model that holds audio
// segments linearly. Nothing outside of this module
// is allowed to mutate segments directly

// ===== Data =====

#[derive(Clone)]
pub struct Segment {
    pub samples: Vec<f32>, // raw audio data (32-bit float samples)
}
// a segment is one recorded chunk
// #[derive(clone)] lets you duplicate a segment
// for recording replacements (retry)

pub struct Project {
    pub segments: Vec<Segment>, // ALL chunks in order
    pub sample_rate: u32, // 44100 or 48000 Hz
    pub channels: u16,    // 1: mono, 2: stereo
}
// persistent timeline of all segments
// added together, once every segment is commited

// ===== State =====

pub enum AppState {
    Idle,
    Recording,
    Reviewing, // keep or retry?
}

pub struct RecorderState {
    pub state: AppState,
    pub current: Option<Segment>, // draft chunks being recorded/reviewed
    pub project: Project, // all chunks
}
// full picture of the app is held inside RecorderState

impl RecorderState {
    pub fn new(sample_rate: u32, channels: u16) -> Self {
        Self {
            state: AppState::Idle,
            current: None,
            project: Project {
                segments: Vec::new(),
                sample_rate,
                channels,
            },
        }
    }

    pub fn start_recording(&mut self) {
        self.state = AppState::Recording;
        self.current = Some(Segment {
            samples: Vec::new(),
        });
    }

    pub fn stop_recording(&mut self) {
        self.state = AppState::Reviewing;
    }

    pub fn approve(&mut self) {
        if let Some(seg) = self.current.take() {
            self.project.segments.push(seg);
        }
        self.state = AppState::Idle;
    }

    pub fn reject(&mut self) {
        self.current = None;
        self.state = AppState::Idle;
    }

    pub fn replace_segment(&mut self, index: usize, new_segment: Segment) {
        self.project.segments[index] = new_segment;
    }

    fn silence(seconds: f32, sample_rate: u32) -> Segment {
        let count = (seconds * sample_rate as f32) as usize;
        Segment {
            samples: vec![0.0; count],
            duration_samples: count,
        }
    }
}
// write logic for RecorderState without audio
// unit test the entire workflow without needing audio
