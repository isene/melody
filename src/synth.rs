//! Audio core: piano-ish tone synthesis, metronome clicks, WAV export and
//! fire-and-forget playback through `aplay` (raw mono s16le 44100). No audio
//! crate — we render PCM ourselves and stream it to the system player, so the
//! sound device only wakes when a note actually plays.

use std::io::Write;
use std::process::{Child, Command, Stdio};

pub const SR: u32 = 44100;

/// MIDI pitch -> frequency (A4 = 69 = 440 Hz).
pub fn freq(pitch: u8) -> f32 {
    440.0 * 2f32.powf((pitch as f32 - 69.0) / 12.0)
}

/// Render one piano-ish note, summed into `buf` starting at sample `at`.
/// A few decaying harmonics under a fast-attack / exponential-decay
/// envelope. `vel` (0..1) scales amplitude. The rendered tail extends
/// past `dur` so notes ring out naturally.
pub fn add_note(buf: &mut [f32], at: usize, pitch: u8, dur: usize, vel: f32) {
    let f = freq(pitch);
    let sr = SR as f32;
    let harm = [1.0f32, 0.6, 0.35, 0.18, 0.08];
    let decay = 3.0 + (pitch as f32 - 60.0).max(0.0) * 0.03;
    let total = dur + (sr * 0.35) as usize;
    for i in 0..total {
        let idx = at + i;
        if idx >= buf.len() {
            break;
        }
        let t = i as f32 / sr;
        let attack = (t / 0.004).min(1.0);
        let env = attack * (-decay * t).exp();
        let mut s = 0.0f32;
        for (h, a) in harm.iter().enumerate() {
            let hf = f * (h as f32 + 1.0);
            if hf > sr / 2.0 {
                break;
            }
            s += a * (2.0 * std::f32::consts::PI * hf * t).sin();
        }
        buf[idx] += s * env * vel * 0.18;
    }
}

/// Render a short metronome click into `buf` at sample `at`. Accent = bar
/// downbeat (higher, louder).
pub fn add_click(buf: &mut [f32], at: usize, accent: bool) {
    let sr = SR as f32;
    let f = if accent { 1600.0 } else { 1000.0 };
    let dur = (sr * 0.04) as usize;
    let amp = if accent { 0.5 } else { 0.32 };
    for i in 0..dur {
        let idx = at + i;
        if idx >= buf.len() {
            break;
        }
        let t = i as f32 / sr;
        let env = (-60.0 * t).exp();
        buf[idx] += amp * env * (2.0 * std::f32::consts::PI * f * t).sin();
    }
}

/// Mix a float buffer down to i16 PCM with soft clipping (tanh).
pub fn to_pcm(buf: &[f32]) -> Vec<i16> {
    buf.iter().map(|&x| (x.tanh() * 32767.0) as i16).collect()
}

/// Write a mono 16-bit 44.1 kHz WAV file.
pub fn write_wav(path: &str, pcm: &[i16]) -> std::io::Result<()> {
    let mut f = std::fs::File::create(path)?;
    let data_len = (pcm.len() * 2) as u32;
    let byte_rate = SR * 2;
    f.write_all(b"RIFF")?;
    f.write_all(&(36 + data_len).to_le_bytes())?;
    f.write_all(b"WAVE")?;
    f.write_all(b"fmt ")?;
    f.write_all(&16u32.to_le_bytes())?;
    f.write_all(&1u16.to_le_bytes())?; // PCM
    f.write_all(&1u16.to_le_bytes())?; // mono
    f.write_all(&SR.to_le_bytes())?;
    f.write_all(&byte_rate.to_le_bytes())?;
    f.write_all(&2u16.to_le_bytes())?; // block align
    f.write_all(&16u16.to_le_bytes())?; // bits
    f.write_all(b"data")?;
    f.write_all(&data_len.to_le_bytes())?;
    let mut bytes = Vec::with_capacity(pcm.len() * 2);
    for s in pcm {
        bytes.extend_from_slice(&s.to_le_bytes());
    }
    f.write_all(&bytes)?;
    Ok(())
}

/// Is `cmd` on PATH? A cheap stat scan, no fork (run once at startup).
fn which(cmd: &str) -> bool {
    std::env::var("PATH").map(|p| {
        p.split(':').any(|d| std::path::Path::new(d).join(cmd).is_file())
    }).unwrap_or(false)
}

/// Pick a raw-PCM player backend: aplay (Linux/ALSA), play (sox, portable),
/// or paplay (PulseAudio). All read mono s16le 44100 from stdin.
fn pick_player() -> Option<(String, Vec<String>)> {
    let opts: [(&str, &[&str]); 3] = [
        ("aplay", &["-q", "-f", "S16_LE", "-c", "1", "-r", "44100", "-t", "raw"]),
        ("play", &["-q", "-t", "raw", "-r", "44100", "-e", "signed", "-b", "16", "-c", "1", "-"]),
        ("paplay", &["--raw", "--format=s16le", "--rate=44100", "--channels=1"]),
    ];
    opts.iter()
        .find(|(c, _)| which(c))
        .map(|(c, a)| (c.to_string(), a.iter().map(|s| s.to_string()).collect()))
}

/// Streams PCM to a system player. The "main" stream (audition / metronome
/// track) is tracked apart from short feedback notes so playback can be
/// toggled and stopped mid-way without the brief monitor notes confusing it.
pub struct Player {
    monitors: Vec<Child>,
    main: Option<Child>,
    backend: Option<(String, Vec<String>)>,
}

impl Player {
    pub fn new() -> Self {
        Player {
            monitors: Vec::new(),
            main: None,
            backend: pick_player(),
        }
    }

    /// True when no audio backend was found (silent mode; export still works).
    pub fn silent(&self) -> bool {
        self.backend.is_none()
    }

    /// Spawn the player and feed the PCM on a detached thread so a long buffer
    /// never blocks the UI. stdin drops at the end -> player sees EOF and exits.
    fn spawn(&self, pcm: Vec<i16>) -> Option<Child> {
        let (prog, args) = self.backend.as_ref()?;
        let mut child = Command::new(prog)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        if let Some(mut stdin) = child.stdin.take() {
            std::thread::spawn(move || {
                let mut bytes = Vec::with_capacity(pcm.len() * 2);
                for s in &pcm {
                    bytes.extend_from_slice(&s.to_le_bytes());
                }
                let _ = stdin.write_all(&bytes);
            });
        }
        Some(child)
    }

    /// Reap finished player processes without blocking.
    pub fn reap(&mut self) {
        self.monitors
            .retain_mut(|c| !matches!(c.try_wait(), Ok(Some(_))));
        if matches!(self.main.as_mut().map(|c| c.try_wait()), Some(Ok(Some(_)))) {
            self.main = None;
        }
    }

    /// Short live-feedback note. Does not count as "playing".
    pub fn note(&mut self, pcm: Vec<i16>) {
        self.reap();
        if let Some(c) = self.spawn(pcm) {
            self.monitors.push(c);
        }
    }

    /// Main playback (audition / metronome track). Replaces any current one.
    pub fn play(&mut self, pcm: Vec<i16>) {
        self.stop();
        self.main = self.spawn(pcm);
    }

    /// Stop main playback only.
    pub fn stop(&mut self) {
        if let Some(mut c) = self.main.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }

    /// Stop everything (record end / program exit).
    pub fn stop_all(&mut self) {
        self.stop();
        for c in &mut self.monitors {
            let _ = c.kill();
            let _ = c.wait();
        }
        self.monitors.clear();
    }
}
