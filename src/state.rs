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
// for recording replacements (retry)

impl Segment {
    // 1 second of pub samples = 44100 indexes (sample_rate)
    #[allow(unused)]
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

// ===== State =====

// tracks the entire app state
#[derive(PartialEq)]
pub enum AppState {
    Idle,
    Recording,
    Reviewing, // keep or retry?
}

// tracks the audio_output thread state
// avoids tangled state with AppState
// e.g. Reviewing + Playing at the same time
#[derive(PartialEq, Clone)]
pub enum PlaybackState {
    Idle,
    Playing, // UI blocks input when playing
}

// separation of event loop and state, the UI dispatches enum commands
// instead of calling methods directly
pub enum Command {
    StartRecording,
    StopRecording,
    Approve,
    Reject,
    RetryCurrentTake,
    PlaySegment(usize), // 0-based index
    PlayAll,
    RetrySegment(usize),
    InsertAfter(usize),
    DeleteSegment(usize),
    Export(String), // path
}

pub struct RecorderState {
    pub state: AppState,
    pub current: Option<Segment>, // current chunk being recorded/reviewed
    pub project: Project, // all chunks
    pub is_insertion: bool, // helps decide between replace vs insert
    pub playback_state: PlaybackState,
}

// holds the the current segment being recorded, the state
// and the project in which it will add the approved recording to
impl RecorderState { // master struct
    pub fn new(sample_rate: u32, channels: u16) -> Self {
        Self {
            state: AppState::Idle,
            current: None, // current recording segment
            is_insertion: false,
            playback_state: PlaybackState::Idle,
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
        self.current = Some(Segment { samples: Vec::new() });
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

    // retry the take that was just recorded
    pub fn retry_current_take(&mut self) {
        if self.state == AppState::Reviewing {
            // Create a new empty segment for the retry
            self.current = Some(Segment { samples: Vec::new() });
            // Switch back to recording from Idle
            self.state = AppState::Recording; // automatically starts recording
            // IMPORTANT: We do NOT reset editing_index or is_insertion here.
            // This ensures we retry the same slot (overwrite/insert/append) 
            // rather than resetting to default append.
        }
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
    #[allow(unused)]
    fn silence(seconds: f32, sample_rate: u32) -> Segment {
        let count = (seconds * sample_rate as f32) as usize;
        Segment {
            samples: vec![0.0; count],
        }
    }

    // *** Helpers ***

    #[allow(unused)]
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

    // is it safe to start recording or playback?
    #[allow(unused)]
    pub fn is_busy(&self) -> bool {
        matches!(self.state, AppState::Recording)
            || self.playback_state == PlaybackState::Playing // PartialEq
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

    fn simulate_recording(recorder: &mut RecorderState, data: Vec<f32>) {
        if let Some(ref mut seg) = recorder.current {
            seg.samples.extend(data);
        }
    }

    #[test]
    fn test_full_workflow() {
        let mut rec = RecorderState::new(44100, 1);

        rec.start_recording();
        simulate_recording(&mut rec, vec![1.0, 2.0, 3.0]);
        rec.stop_recording();
        rec.approve();
        assert_eq!(rec.get_segment_count(), 1);

        rec.start_recording();
        simulate_recording(&mut rec, vec![4.0, 5.0]);
        rec.stop_recording();
        rec.approve();
        assert_eq!(rec.get_segment_count(), 2);

        rec.delete_segment(0);
        assert_eq!(rec.get_segment_count(), 1);
        assert_eq!(rec.project.segments[0].samples, vec![4.0, 5.0]);
    }

    #[test]
    fn test_retry_logic() {
        let mut rec = RecorderState::new(44100, 1);

        rec.start_recording();
        simulate_recording(&mut rec, vec![1.0]);
        rec.stop_recording();
        rec.approve();

        rec.retry_segment(0);
        simulate_recording(&mut rec, vec![9.9]);
        rec.stop_recording();
        rec.approve();

        assert_eq!(rec.project.segments[0].samples, vec![9.9]);
        assert_eq!(rec.get_segment_count(), 1);
    }

    #[test]
    fn test_insert_logic() {
        let mut rec = RecorderState::new(44100, 1);

        rec.start_recording();
        simulate_recording(&mut rec, vec![1.0]);
        rec.stop_recording();
        rec.approve();

        rec.insert_segment(0);
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

    // test the Command enum dispatch these are only
    // commands that don't involve audio I/O can be tested directly
    #[test]
    fn test_command_dispatch() {
        let mut rec = RecorderState::new(44100, 1);

        dispatch_command(&mut rec, Command::StartRecording);
        simulate_recording(&mut rec, vec![0.5]);
        dispatch_command(&mut rec, Command::StopRecording);
        dispatch_command(&mut rec, Command::Approve);
        assert_eq!(rec.get_segment_count(), 1);

        dispatch_command(&mut rec, Command::DeleteSegment(0));
        assert_eq!(rec.get_segment_count(), 0);
    }
}

// state dispatch, called by main, no audio I/O, no threads
// Note: audio_output commands(PlaySegment, PlayAll) are handled in main
// because they need hold Arc<Mutex<RecorderState>> + threads and file I/O
pub fn dispatch_command(rec: &mut RecorderState, cmd: Command) {
    match cmd {
        Command::StartRecording       => rec.start_recording(),
        Command::StopRecording        => rec.stop_recording(),
        Command::Approve              => rec.approve(),
        Command::Reject               => rec.reject(),
        Command::RetryCurrentTake     => rec.retry_current_take(),
        Command::RetrySegment(i)      => { rec.retry_segment(i); }
        Command::InsertAfter(i)       => { rec.insert_segment(i); }
        Command::DeleteSegment(i)     => { rec.delete_segment(i); }
        _ => {}
    }
}
