# melody

<img src="img/melody.svg" align="right" width="150">

**Terminal melody maker. Written in Rust.**

![Rust](https://img.shields.io/badge/language-Rust-f74c00) ![License](https://img.shields.io/badge/license-Unlicense-green) ![Platform](https://img.shields.io/badge/platform-Linux%20%7C%20macOS-blue) ![Stay Amazing](https://img.shields.io/badge/Stay-Amazing-important)

Compose a single piano line on a piano-roll, then export a clean WAV to use as the basis for a [Suno](https://suno.com) track. Play freely, record what you play (quantized to a grid, against a metronome), then tweak each note's pitch, length and strength. Built on [crust](https://github.com/isene/crust). Part of the [Fe₂O₃ Rust terminal suite](https://github.com/isene/fe2o3).

No audio crate: notes are synthesised to PCM and streamed to a system player (`aplay`, `play`/sox, or `paplay`), so the sound device only wakes when something actually plays. WAV is written by hand.

## Build

```bash
cargo build --release
```

Sound needs one of `aplay` (alsa-utils), `play` (sox) or `paplay` on the PATH. WAV export works without any of them.

## Three modes

- **EDIT** (the piano-roll): move, lengthen, shorten, restrengthen or delete each note.
- **PLAY** (`Tab`): jam freely on the keyboard piano; nothing is captured.
- **REC** (`r`): play along to the metronome; keystrokes are captured and
  quantized to the grid. A one-bar count-in plays first when the metronome is
  on. `Esc`/`Enter` stops and drops you back in EDIT to tweak the take.

## Keys

### Edit
| Key | Action |
|---|---|
| `h j k l` / arrows | move cursor (time / pitch) |
| `Enter` | add a note at the cursor (or audition the note under it) |
| `H` / `L` | move selected note earlier / later |
| `K` / `J` | move selected note up / down a semitone |
| `+` / `-` | lengthen / shorten selected note (down to one tick) |
| `1`–`9` | note strength, soft→loud (also sets its colour) |
| `x` / `Del` | delete selected note |
| `Space` | play / stop the melody (a playhead follows along) |
| `r` | record · `Tab` jam · `m` metronome on/off |
| `g` | cycle grid (1/4, 1/8, 1/16) |
| `,` / `.` | tempo −/+ 5 BPM |
| `w` / `o` | save / open a `.mel` file |
| `e` | export WAV |
| `?` | keybinding help · `n` new · `q` quit |

### Play / Record (the keyboard piano)
| Key | Note |
|---|---|
| `z s x d c v g b h n j m ,` | C C# D D# E F F# G G# A A# B C |
| `[` / `]` | octave down / up |

Each note's colour shows its strength (cool = soft, hot = loud). While the
melody plays, a cyan playhead marks the current position and the view scrolls
to follow it; press any key to stop.

## Make a Suno track

1. Find the melody in **PLAY**, then `r` to **record** a take to the click.
2. Tweak notes in **EDIT** (`H/L/K/J`, `+/-`, `1`–`9`, `x`).
3. `e` to **export** a WAV.
4. Upload the WAV to Suno as the basis for your song.

## Headless render

```bash
melody --render song.mel song.wav
```

## File format (`.mel`)

Plain text, one directive per line:

```
melody 1
bpm 120
sig 4 4
tpb 4
step 2
note 60 0 2 0.80    # pitch(MIDI) start(ticks) length(ticks) strength(0..1)
```

## License

Unlicense (public domain).
