A voice-over audio application that records your voice in manageable chunks with a built-in review workflow. This tool is designed for situations where you need to record long-form content-like narration, podcasts, or voice-overs without the pressure of getting everything perfect in a single continuous take.

## The problem this solves

Traditional recording software forces you to choose between two frustrating workflows: either record everything in one giant file and edit out the mistakes later(which can take hours), or stop and start the recording repeatedly, which becomes tedious. This project offers a middle path. You record in short segments, review each one immediately, and decide on the spot whether to keep or redo the segment with a single keypress. The good takes get appended to your project automatically. No need to mess around with the audio timeline.

You then just throw the exported output into Audacity(or your preferred editor) and apply your usual effects in a single pas without any further editing.


## Command Summary

| Key / Command   | Action       | Description                                       |
| -------------   | ------------ | ------------------------------------------------- |
| `r`             | Record       | Record a new segment and append it to the end.    |
| `p`             | Play         | Play the last recorded segment.                   |
| `p <n>`         | Play segment | Play segment number n.                            |
| `pa`            | Play all     | Play all segments in sequence (the full project). |
| `retry <n>`     | Retry        | Re-record segment number n.                       |
| `insert <n>`    | Insert       | Insert a new segment after position n.            |
| `c`             | Confirm      | Approve the current segment.                      |
| `x`             | Reject       | Reject the current segment.                       |
| `e`             | Export       | Export all confirmed segments and exit.           |