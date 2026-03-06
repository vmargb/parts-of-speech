A non-linear voice-over application that records your voice in manageable chunks with a built-in review workflow. This tool is designed for situations where you need to record long-form content-like narration, podcasts, or voice-overs without the pressure of getting everything perfect in a single continuous take.

## The problem this solves

Traditional recording software forces you to choose between two frustrating workflows: either record everything in one giant file and edit out the mistakes later(which can take hours), or stop and start the recording repeatedly, which becomes tedious. This project offers a middle path. You record in short segments, review each one immediately, and decide on the spot whether to keep or redo the segment with a single keypress. The good takes get appended to your project automatically. No need to mess around with the audio timeline.

You then just throw the exported output into Audacity(or preferred editor) and apply your effects in one go without further editing needed.


---

## Dependencies

Install Rust with your package-manager or from [rustup.rs](https://rustup.rs)

**Linux**: Install ALSA development libraries (required for audio):

| Distro          | Command                                      |
|-----------------|----------------------------------------------|
| Ubuntu/Debian   | `sudo apt install libasound2-dev pkg-config` |
| Fedora          | `sudo dnf install alsa-lib-devel pkgconf`    |
| Arch            | `sudo pacman -S alsa-lib pkgconf`            |

**macOS**: Install Xcode Command Line Tools: `xcode-select --install`  
*(Uses CoreAudio—no ALSA needed)*

**Windows**: Install [Visual Studio Build Tools](https://visualstudio.microsoft.com/downloads/#build-tools-for-visual-studio-2022)
*(Uses WASAPI—no ALSA needed)*

## Quick Start

```bash
git clone https://github.com/vmargb/parts-of-speech.git
cd parts-of-speech
cargo run
```

---

## Command Summary

| Key / Command    | Action       | Description                                       |
| -------------    | ------------ | ------------------------------------------------- |
| `r`              | Record       | Record a new segment                              |
| `s`              | Stop         | Stop recording to review the segment.             |
| `c`              | Confirm      | Approve the current segment.                      |
| `x`              | Reject       | Reject the current segment.                       |
| `t`              | Try again    | Reject the current segment and try again          |
| `p`              | Play         | Play the last recorded segment.                   |
| `p <n>`          | Play segment | Play segment number n.                            |
| `pa`             | Play all     | Play all segments in sequence (the full project). |
| `retry <n>`      | Retry        | Re-record segment number n.                       |
| `delete <n>`     | Delete       | Delete segment number n.                          |
| `insert <n>`     | Insert       | Insert a new segment after position n.            |
| `trim s/e <secs>`| Trim         | Trims the start and end of the segment by <secs>. |
| `e`              | Export       | Export all confirmed segments and exit.           |


### Workflow

1. **Record a Segment**:
   - Use the `r` command to start recording a new segment.

2. **Review the Segment**:
   - While recording, you have several options:
     - Press `c` to **confirm** the segment if you are satisfied with it.
     - Press `x` to **reject** the segment if you know it was bad.
     - Press `t` to **reject and try again** if you want to reject the current segment and instantly record a new one.

3. **Stop and Review (To listen again before deciding):**
   - Use the `s` command to stop recording which auto-plays the recorded segment for you. This allows you to perform the actions listed in step 2.

4. **Repeat:**
   - Repeat steps 1-3 until you have recorded all the necessary segments.

5. **Playback:**
   - Use `pa` to listen to the entire project or `p <n>` to play a specific segment.

6. **Edit Segments:**
   - Use `retry <n>`, `delete <n>`, and `insert <n>` to make any necessary adjustments to your segments.

7. **Export:**
   - Once you are satisfied with all segments, use the `e` command to export all confirmed segments and exit the application.