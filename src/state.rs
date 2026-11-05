use serde::{Serialize, Deserialize};

// This module is the data model that holds audio
// segments linearly. Nothing outside of this module
// is allowed to mutate segments directly

// ===== Data =====

#[derive(Clone, Serialize, Deserialize)]
pub struct Segment { // a single recording take
    // the actual audio numbers
    pub samples: Vec<f32>, // raw audio data (32-bit float samples)
    // if true, this segment was created as intentional silence (not a recording).
    // silence segments render differently in the GUI and support expansion (adding more
    // silence) in addition to trimming. Serialised so projects round-trip correctly.
    pub is_silence: bool,
    // projects are navigable by optional name instead of by index alone. None = unnamed.
    pub label: Option<String>,
}

pub const MAX_LABEL_CHARS: usize = 60; // before turning into ellipses

// bincode is positional, so adding/removing a field breaks old files
// new saves start with this 8 byte header
const PROJECT_MAGIC: &[u8; 8] = b"POSPRJ02";

// project layout from before labels existed, only used to open old save files
#[derive(Serialize, Deserialize)]
struct SegmentV1 { samples: Vec<f32>, is_silence: bool }
#[derive(Serialize, Deserialize)]
struct ProjectV1 {
    segments: Vec<SegmentV1>,
    sample_rate: u32,
    channels: u16,
    editing_index: Option<usize>,
}
impl From<ProjectV1> for Project {
    fn from(old: ProjectV1) -> Self {
        Project {
            segments: old.segments.into_iter()
                .map(|s| Segment { samples: s.samples, is_silence: s.is_silence, label: None })
                .collect(),
            sample_rate: old.sample_rate,
            channels: old.channels,
            editing_index: old.editing_index,
        }
    }
}
// a segment is one recorded chunk
// for recording replacements (retry)

impl Segment {
    // 1 second of pub samples = 48000 indexes (sample_rate)
    #[allow(unused)]
    pub fn duration_seconds(&self, sample_rate: u32) -> f32 {
        self.samples.len() as f32 / sample_rate as f32
    }
}


#[derive(Clone, Serialize, Deserialize)]
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
    PlaySegment(usize),
    PlayAll,
    // play the project starting at the given segment index (0-based)
    // PlayAll is equivalent to PlayFrom(0)
    PlayFrom(usize),
    RetrySegment(usize),
    InsertAfter(usize),
    DeleteSegment(usize),
    // name (or with None / empty text, un-name) the segment at the given index
    RenameSegment(usize, Option<String>),
    // MoveSegment(from, to), take the segment at `from` and put it at index `to`
    MoveSegment(usize, usize),
    TrimStart(Option<usize>, f32),
    TrimEnd(Option<usize>, f32),   // (index, seconds) - None = current
    // insert a silence segment of `seconds` duration after the given index
    InsertSilenceAfter(usize, f32),
    // Add `seconds` of extra silence to an existing silence segment at the given index
    ExpandSilence(usize, f32),
    // signals the audio thread to stop playback immediately
    StopPlayback,
    Undo,
    Redo,
    Export(Option<String>), // None = use auto-path, Some = use explicit path
    LoadProject(String),
    SaveProjectAs(String),
}

pub struct RecorderState {
    pub state: AppState,
    pub current: Option<Segment>, // current chunk being recorded/reviewed
    pub project: Project, // all chunks
    pub is_insertion: bool, // helps decide between replace vs insert
    pub playback_state: PlaybackState,
    // project history
    // automatically triggered by edit methods
    pub history: Vec<Project>, // backup for commited segments
    pub history_index: usize, // current position in history
    pub previous_current: Option<Segment>, // backup for uncommitted segment
    pub next_current: Option<Segment>, // redo backup for uncommitted segment
    pub save_path: Option<String>, // where the project is saved
    pub pre_rolling: bool, // true while pre-roll cue is playing
}

// holds the the current segment being recorded, the state
// and the project in which it will add the approved recording to
impl RecorderState { // master struct
    pub fn new(sample_rate: u32, channels: u16) -> Self {
        let empty_project = Project {
            segments: Vec::new(),
            sample_rate,
            channels,
            editing_index: None,
        };

        Self {
            state: AppState::Idle,
            current: None,
            is_insertion: false,
            playback_state: PlaybackState::Idle,
            project: empty_project.clone(), 
            // capture the initial state to be able to undo back to an empty project
            history: vec![empty_project], 
            history_index: 0,
            previous_current: None,
            next_current: None,
            save_path: None,
            pre_rolling: false,
        }
    }

    // --Save & Load Functionality ---------------------------------

    pub fn set_save_path(&mut self, path: String) {
        self.save_path = Some(path);
    }

    // *** Project History
    // save current project state to history (call BEFORE modifying)
    fn save_state(&mut self) {
        // truncate history beyond current index (discard redo branch)
        // check if history is non-empty before subtracting to prevent usize underflow
        if !self.history.is_empty() && self.history_index < self.history.len() - 1 {
            self.history.truncate(self.history_index + 1);
        }
        // clone and push current project state
        self.history.push(self.project.clone());
        self.history_index = self.history.len() - 1;
        
        const MAX_HISTORY: usize = 5; // limit history size
        if self.history.len() > MAX_HISTORY {
            self.history.remove(0); // remove oldest
            if self.history_index > 0 {
                self.history_index -= 1; // reposition one back
            }
        }
        
        self.next_current = None; // clear redo backup for current segment
        self.save_to_disk(); // trigger auto-save to disk
    }

    pub fn save_to_disk(&self) {
        if let Some(path) = &self.save_path {
            let _ = self.write_project(path);
        }
    }

    // header + bincode, streamed straight to the file so a long project isn't copied in RAM twice
    fn write_project(&self, path: &str) -> std::io::Result<()> {
        use std::io::Write;
        let mut w = std::io::BufWriter::new(std::fs::File::create(path)?);
        w.write_all(PROJECT_MAGIC)?;
        bincode::serialize_into(&mut w, &self.project)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        w.flush()
    }

    fn decode_project(data: &[u8]) -> Result<Project, Box<dyn std::error::Error>> {
        match data.strip_prefix(PROJECT_MAGIC) {
            Some(body) => Ok(bincode::deserialize(body)?),
            // no header: saved before labels existed
            None => Ok(bincode::deserialize::<ProjectV1>(data)?.into()),
        }
    }

    pub fn load_from_disk(&mut self, path: String) -> Result<(), Box<dyn std::error::Error>> {
        let data = std::fs::read(&path)?;
        let project = Self::decode_project(&data)?;
        
        self.project = project.clone();
        self.save_path = Some(path);
        
        // reset history on load
        self.history = vec![project];
        self.history_index = 0;
        self.previous_current = None;
        self.next_current = None;
        
        Ok(())
    }

    // save the current uncommitted segment to backup (call BEFORE modifying)
    fn push_current_backup(&mut self) {
        self.next_current = None; // clear redo backup when state is changed
        if let Some(seg) = &self.current {
            self.previous_current = Some(seg.clone());
        }
    }

    // this does two things:
    // if there's an uncommited backup segment, switch it
    // if there's a previous project copy, switch to it
    pub fn undo(&mut self) -> bool {
        // case 1: undo uncommitted segment (e.g. trim during Reviewing)
        if let Some(prev) = self.previous_current.take() {
            if let Some(curr) = &self.current {
                self.next_current = Some(curr.clone());
            }
            self.current = Some(prev);
            return true;
        }

        // case 2: undo project state
        if self.history_index > 0 {
            self.history_index -= 1;
            self.project = self.history[self.history_index].clone();
            return true;
        }

        false
    }

    pub fn redo(&mut self) -> bool {
        // case 1: redo uncommitted segment
        if let Some(next) = self.next_current.take() {
            if let Some(curr) = &self.current {
                self.previous_current = Some(curr.clone());
            }
            self.current = Some(next);
            return true;
        }

        // case 2: redo project state
        if self.history_index < self.history.len() - 1 {
            self.history_index += 1;
            self.project = self.history[self.history_index].clone();
            return true;
        }

        false
    }

    // The last `secs` seconds of audio that will sit directly *before* the take
    // that is about to be recorded, for pre-roll cueing
    pub fn pre_roll_samples(&self, secs: f32) -> Vec<f32> {
        let want = (secs.max(0.0) * self.project.sample_rate as f32) as usize;
        let end = self.project.editing_index
            .unwrap_or(self.project.segments.len())
            .min(self.project.segments.len());
        if want == 0 || end == 0 { return Vec::new(); }

        let mut chunks: Vec<&[f32]> = Vec::new(); // collected newest-first
        let mut got = 0usize;
        for seg in self.project.segments[..end].iter().rev() {
            if got >= want { break; }
            let take = (want - got).min(seg.samples.len());
            chunks.push(&seg.samples[seg.samples.len() - take..]);
            got += take;
        }
        chunks.iter().rev().flat_map(|c| c.iter().copied()).collect()
    }

    // *** Workflow Methods ***

    // create an empty segment and start recording
    pub fn start_recording(&mut self) {
        self.state = AppState::Recording;
        self.is_insertion = false; // append not insert
        self.current = Some(Segment { samples: Vec::new(), is_silence: false, label: None });
        self.project.editing_index = None; // None: segment at end default
        // starting a new take invalidates old undo history for the previous take
        self.previous_current = None;
        self.next_current = None;
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
                    } else { // if replace, replace (a retake keeps the slot's name)
                        let mut seg = seg;
                        if seg.label.is_none() {
                            seg.label = self.project.segments[idx].label.take();
                        }
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
        // once approved, clear current-segment undo buffers when lifecycle ends
        self.previous_current = None;
        self.next_current = None;
    }
    // retry can provide an idx number, which is held in struct
    // so approve accounts for both cases

    pub fn reject(&mut self) {
        self.current = None; // delete current segment
        self.project.editing_index = None;
        self.state = AppState::Idle;
        // once rejected, clear current-segment undo buffers
        self.previous_current = None;
        self.next_current = None;
    }

    // retry the take that was just recorded
    pub fn retry_current_take(&mut self) {
        if self.state == AppState::Reviewing {
            // Create a new empty segment for the retry
            self.current = Some(Segment { samples: Vec::new(), is_silence: false, label: None });
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
        self.current = Some(Segment { samples: Vec::new(), is_silence: false, label: None });
        self.state = AppState::Recording;
        true
    }

    // insert a new recording after the index
    pub fn insert_segment(&mut self, after_index: usize) -> bool {
        if after_index >= self.project.segments.len() { return false; }
        
        self.project.editing_index = Some(after_index + 1); // index after
        self.is_insertion = true; // inserting
        self.current = Some(Segment { samples: Vec::new(), is_silence: false, label: None });
        self.state = AppState::Recording;
        true
    }

    // removes a segment
    pub fn delete_segment(&mut self, index: usize) -> bool {
        if index >= self.project.segments.len() { return false; }
        self.project.segments.remove(index);
        true
    }

    // silence segment of exactly `seconds` duration.
    pub fn make_silence(seconds: f32, sample_rate: u32) -> Segment {
        let count = (seconds * sample_rate as f32) as usize;
        Segment {
            samples: vec![0.0; count],
            is_silence: true,
            label: None,
        }
    }

    // insert a silence segment of `seconds` duration after `after_index`
    pub fn insert_silence_after(&mut self, after_index: usize, seconds: f32) -> bool {
        if after_index >= self.project.segments.len() { return false; }
        let sr = self.project.sample_rate;
        let seg = Self::make_silence(seconds, sr);
        self.project.segments.insert(after_index + 1, seg);
        true
    }

    // append `extra_seconds` of additional silence to an existing silence segment
    // return false if the segment at `index` is not a silence segment
    pub fn expand_silence(&mut self, index: usize, extra_seconds: f32) -> bool {
        if index >= self.project.segments.len() { return false; }
        let sr = self.project.sample_rate;
        let seg = &mut self.project.segments[index];
        if !seg.is_silence { return false; }
        let extra = (extra_seconds * sr as f32) as usize;
        seg.samples.extend(vec![0.0_f32; extra]);
        true
    }

    // reorder, remove the segment at `from` and re-insert it so it ends up at index `to`
    // only while Idle
    pub fn move_segment(&mut self, from: usize, to: usize) -> bool {
        let len = self.project.segments.len();
        if self.state != AppState::Idle || from >= len || to >= len || from == to {
            return false;
        }
        let seg = self.project.segments.remove(from);
        self.project.segments.insert(to, seg);
        true
    }

    // set or clear a segment's label. Whitespace is trimmed, an empty label clears it
    // returns false (and changes nothing) if the index is invalid or the label is unchanged
    pub fn rename_segment(&mut self, index: usize, label: Option<String>) -> bool {
        let cleaned = label
            .map(|l| l.trim().chars().take(MAX_LABEL_CHARS).collect::<String>())
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty());
        let Some(seg) = self.project.segments.get_mut(index) else { return false };
        if seg.label == cleaned { return false; }
        seg.label = cleaned;
        true
    }

    pub fn trim_start(&mut self, segment_index: Option<usize>, seconds: f32) -> bool {
        let sample_rate = self.project.sample_rate;
        let samples_to_trim = (seconds * sample_rate as f32) as usize;

        if samples_to_trim == 0 { return false; }

        match segment_index {
            None => { // current segment
                self.push_current_backup(); // save backup
                if let Some(ref mut seg) = self.current {
                    if samples_to_trim >= seg.samples.len() {
                        seg.samples.clear(); // Trim all if requested >= length
                    } else {
                        seg.samples.drain(0..samples_to_trim);
                    }
                    return true;
                }
            }
            Some(idx) => {
                self.save_state(); // save backup (not current segment so save state)
                if idx < self.project.segments.len() {
                    let seg = &mut self.project.segments[idx];
                    if samples_to_trim >= seg.samples.len() {
                        seg.samples.clear();
                    } else {
                        seg.samples.drain(0..samples_to_trim);
                    }
                    return true;
                }
            }
        }
        false
    }

    pub fn trim_end(&mut self, segment_index: Option<usize>, seconds: f32) -> bool {
        let sample_rate = self.project.sample_rate;
        let samples_to_trim = (seconds * sample_rate as f32) as usize;

        if samples_to_trim == 0 { return false; }

        match segment_index {
            None => {
                // Trim current segment (during Reviewing)
                self.push_current_backup(); // save backup
                if let Some(ref mut seg) = self.current {
                    if samples_to_trim >= seg.samples.len() {
                        seg.samples.clear();
                    } else {
                        let new_len = seg.samples.len() - samples_to_trim;
                        seg.samples.truncate(new_len);
                    }
                    return true;
                }
            }
            Some(idx) => {
                // Trim committed segment in project
                self.save_state(); // save backup
                if idx < self.project.segments.len() {
                    let seg = &mut self.project.segments[idx];
                    if samples_to_trim >= seg.samples.len() {
                        seg.samples.clear();
                    } else {
                        let new_len = seg.samples.len() - samples_to_trim;
                        seg.samples.truncate(new_len);
                    }
                    return true;
                }
            }
        }
        false
    }

    // *** Helpers ***

    pub fn total_duration(&self) -> String {
        let total_secs: f32 = self.project.segments
            .iter()
            .map(|seg| seg.duration_seconds(self.project.sample_rate))
            .sum();
        let mins = (total_secs / 60.0) as u32;
        let secs = (total_secs % 60.0) as u32;
        format!("{}m:{:02}s", mins, secs)
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
        let mut rec = RecorderState::new(48000, 1);

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
        let mut rec = RecorderState::new(48000, 1);

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
        let mut rec = RecorderState::new(48000, 1);

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
        let mut rec = RecorderState::new(48000, 1);
        rec.start_recording();
        simulate_recording(&mut rec, vec![1.0, 1.0, 1.0]);
        rec.stop_recording();
        rec.reject();

        assert_eq!(rec.get_segment_count(), 0);
        assert!(rec.current.is_none());
    }

    #[test]
    fn test_silence_insert_and_expand() {
        let mut rec = RecorderState::new(48000, 1);

        // record two segments
        rec.start_recording();
        simulate_recording(&mut rec, vec![1.0; 48000]); // 1s
        rec.stop_recording();
        rec.approve();

        rec.start_recording();
        simulate_recording(&mut rec, vec![2.0; 48000]); // 1s
        rec.stop_recording();
        rec.approve();

        // insert 0.5s silence between them
        let ok = rec.insert_silence_after(0, 0.5);
        assert!(ok);
        assert_eq!(rec.get_segment_count(), 3);
        assert!(rec.project.segments[1].is_silence);
        assert_eq!(rec.project.segments[1].samples.len(), 24000); // 0.5 * 48000

        // expand that silence by another 0.5s -> 1s total
        let ok = rec.expand_silence(1, 0.5);
        assert!(ok);
        assert_eq!(rec.project.segments[1].samples.len(), 48000); // 1.0 * 48000

        // expanding a non-silence segment should fail
        let ok = rec.expand_silence(0, 0.5);
        assert!(!ok);
    }

    #[test]
    fn test_silence_is_flagged() {
        let seg = RecorderState::make_silence(2.0, 44100);
        assert!(seg.is_silence);
        assert_eq!(seg.samples.len(), 88200);
        assert!(seg.samples.iter().all(|&s| s == 0.0));
    }

    // test the Command enum dispatch these are only
    // commands that don't involve audio I/O can be tested directly
    #[test]
    fn test_command_dispatch() {
        let mut rec = RecorderState::new(48000, 1);

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
// Note: audio_output commands(PlaySegment, PlayAll, PlayFrom) are handled in main
// because they need hold Arc<Mutex<RecorderState>> + threads and file I/O
pub fn dispatch_command(rec: &mut RecorderState, cmd: Command) {
    match cmd {
        Command::StartRecording           => rec.start_recording(),
        Command::StopRecording            => rec.stop_recording(),
        Command::Approve                  => { rec.approve(); rec.save_state(); }
        Command::Reject                   => rec.reject(),
        Command::RetryCurrentTake         => rec.retry_current_take(), // saved in prev_current
        Command::RetrySegment(i)          => { rec.retry_segment(i); rec.save_state(); }
        Command::InsertAfter(i)           => { rec.insert_segment(i); rec.save_state(); }
        Command::DeleteSegment(i)         => { rec.delete_segment(i); rec.save_state(); }
        Command::TrimStart(idx, secs)     => { rec.trim_start(idx, secs); } // saved in prev_current
        Command::TrimEnd(idx, secs)       => { rec.trim_end(idx, secs); }
        Command::InsertSilenceAfter(i, s) => { rec.save_state(); rec.insert_silence_after(i, s); }
        Command::ExpandSilence(i, s)      => { rec.save_state(); rec.expand_silence(i, s); }
        Command::MoveSegment(from, to)    => {
            if rec.move_segment(from, to) { rec.save_state(); }
            else { eprintln!("[move] rejected {} -> {} (segments: {}, idle: {})",
                             from, to, rec.project.segments.len(), rec.state == AppState::Idle); }
        }
        Command::RenameSegment(i, label)  => { if rec.rename_segment(i, label) { rec.save_state(); } }
        Command::StopPlayback             => {} // handled in main.rs via AtomicBool
        Command::Undo                     => { rec.undo(); }
        Command::Redo                     => { rec.redo(); }
        _ => {}
    }
}
