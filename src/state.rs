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
    pub is_insertion: bool, // helps decide between replace vs insert
}
// full picture of the state is held inside RecorderState


// holds the the current segment being recorded, the state
// and the project in which it will add the approved recording to
impl RecorderState { // master struct
    pub fn new(sample_rate: u32, channels: u16) -> Self {
        Self {
            state: AppState::Idle,
            current: None, // current recording segment
            is_insertion: false,
            project: Project {
                segments: Vec::new(),
                sample_rate,
                channels,
                editing_index: None,
            },
        }
    }

    // *** Workflow Methods ***

    // create an empty segment and start recording
    pub fn start_recording(&mut self) {
        self.state = AppState::Recording;
        self.is_insertion = false; // append not insert
        self.current = Some(Segment {
            samples: Vec::new(),
        });
        self.project.editing_index = None; // None: segment at end default
    }

    pub fn stop_recording(&mut self) {
        if let AppState::Recording = self.state {
            self.state = AppState::Reviewing; // only review if we were recording
        }
    }

    // appends the approved segment into project.segments
    pub fn approve(&mut self) {
        if let Some(seg) = self.current.take() { // if current segment exists
            match self.project.editing_index.take() { // if index is provided
                Some(idx) if idx <= self.project.segments.len() => { // in bound
                    if self.is_insertion { // if insert, slide it in
                        self.project.segments.insert(idx, seg);
                    } else { // if replace, replace
                        self.project.segments[idx] = seg;
                    }
                }
                _ => { // default: just append to the end
                    self.project.segments.push(seg);
                }
            }
        }
        self.state = AppState::Idle;
        self.is_insertion = false;
    }
    // retry can provide an idx number, which is held in struct
    // so approve accounts for both cases

    pub fn reject(&mut self) {
        self.current = None; // delete current segment
        self.project.editing_index = None;
        self.state = AppState::Idle;
    }

    // *** Edit Methods ***
    // re record a specific segment (0-based)
    pub fn retry_segment(&mut self, index: usize) -> bool {
        if index >= self.project.segments.len() { return false; }
        
        self.project.editing_index = Some(index);
        self.is_insertion = false; // overwriting
        self.current = Some(Segment { samples: Vec::new() });
        self.state = AppState::Recording;
        true
    }

    // insert a new recording after the index
    pub fn insert_segment(&mut self, after_index: usize) -> bool {
        if after_index >= self.project.segments.len() { return false; }
        
        self.project.editing_index = Some(after_index + 1); // index after
        self.is_insertion = true; // inserting
        self.current = Some(Segment { samples: Vec::new() });
        self.state = AppState::Recording;
        true
    }

    // removes a segment
    pub fn delete_segment(&mut self, index: usize) -> bool {
        if index >= self.project.segments.len() { return false; }
        self.project.segments.remove(index);
        true
    }

    // optionally add empty segments in between recordings
    // silence(0.5, sample_rate) would add a 0.5s silence
    fn silence(seconds: f32, sample_rate: u32) -> Segment {
        let count = (seconds * sample_rate as f32) as usize;
        Segment {
            samples: vec![0.0; count],
        }
    }

    // *** Helpers ***

    pub fn total_duration(&self) -> f32 {
        self.project.segments
            .iter()
            .map(|seg| seg.duration_seconds(self.project.sample_rate))
            .sum()
    }

    pub fn get_segment(&self, index: usize) -> Option<&Segment> {
        self.project.segments.get(index) // 0-based
    }

    pub fn get_segment_count(&self) -> usize {
        self.project.segments.len()
    }
}
// write logic for RecorderState without audio
// unit test the entire workflow without needing audio


// -------------------------
// Tests
// -------------------------
#[cfg(test)]
mod tests {
    use super::*;

    // simulate recording audio data without mic
    fn simulate_recording(recorder: &mut RecorderState, data: Vec<f32>) {
        if let Some(ref mut seg) = recorder.current {
            seg.samples.extend(data);
        }
    }

    #[test]
    fn test_full_workflow() {
        let mut rec = RecorderState::new(44100, 1);

        // 1. Record first segment
        rec.start_recording();
        simulate_recording(&mut rec, vec![1.0, 2.0, 3.0]);
        rec.stop_recording();
        rec.approve();
        assert_eq!(rec.get_segment_count(), 1);

        // 2. Record second segment
        rec.start_recording();
        simulate_recording(&mut rec, vec![4.0, 5.0]);
        rec.stop_recording();
        rec.approve();
        assert_eq!(rec.get_segment_count(), 2);

        // 3. Test Delete
        rec.delete_segment(0); // Delete the [1,2,3] segment
        assert_eq!(rec.get_segment_count(), 1);
        assert_eq!(rec.project.segments[0].samples, vec![4.0, 5.0]);
    }

    #[test]
    fn test_retry_logic() {
        let mut rec = RecorderState::new(44100, 1);
        
        // Setup: Add one segment [1.0]
        rec.start_recording();
        simulate_recording(&mut rec, vec![1.0]);
        rec.stop_recording();
        rec.approve();

        // Retry index 0
        rec.retry_segment(0);
        simulate_recording(&mut rec, vec![9.9]); // New data
        rec.stop_recording();
        rec.approve();

        assert_eq!(rec.project.segments[0].samples, vec![9.9]);
        assert_eq!(rec.get_segment_count(), 1); // Count shouldn't change
    }

    #[test]
    fn test_insert_logic() {
        let mut rec = RecorderState::new(44100, 1);
        
        // add initial segment [1.0] at index 0
        rec.start_recording();
        simulate_recording(&mut rec, vec![1.0]);
        rec.stop_recording();
        rec.approve();

        // insert AFTER segment 1 (0 internally)
        // user typed 'insert 1', so we call insert_segment(1-1)
        rec.insert_segment(1-1); 
        simulate_recording(&mut rec, vec![2.0]);
        rec.stop_recording();
        rec.approve();

        assert_eq!(rec.get_segment_count(), 2);
        assert_eq!(rec.project.segments[0].samples, vec![1.0]);
        assert_eq!(rec.project.segments[1].samples, vec![2.0]);
    }

    #[test]
    fn test_reject_recording() {
        let mut rec = RecorderState::new(44100, 1);
        rec.start_recording();
        simulate_recording(&mut rec, vec![1.0, 1.0, 1.0]);
        rec.stop_recording();
        rec.reject();

        assert_eq!(rec.get_segment_count(), 0);
        assert!(rec.current.is_none());
    }
}
