# Changelog

## 0.1.0

Initial release.

- Piano-roll TUI (pitch vertical, time horizontal, bar lines).
- Three modes: PLAY (free jam), REC (real-time capture, quantized to grid,
  metronome with one-bar count-in), EDIT (per-note move/lengthen/shorten/delete).
- Computer-keyboard piano input (`z s x d c v g b h n j m ,`, `[`/`]` octave).
- Toggleable metronome; heard during record and audition.
- Per-note strength (velocity), keys `1`-`9`; each note's colour tracks its
  strength on a soft→loud heat ramp.
- `Space` plays with a moving playhead that the view follows; any key stops it.
- Note length adjusts by the grid step, down to a single tick (shorter than the
  default), and `+`/`-` are reversible.
- `?` modal keybinding help.
- Self-synthesised piano tone (harmonics + ADSR), streamed to `aplay`.
- WAV export for Suno; `.mel` text save/open (note line carries velocity);
  `--render` headless mode.
