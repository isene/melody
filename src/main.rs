//! melody — a terminal melody maker for the Fe2O3 suite.
//!
//! Three states: PLAY (jam freely on the computer-keyboard piano), REC
//! (play along to a metronome; keystrokes are captured, quantized to the
//! grid), and EDIT (a piano-roll where each note can be moved in pitch/time,
//! lengthened, shortened or deleted). Export a clean WAV to use as the basis
//! for a fuller track in whatever tool you like.

mod synth;

use crust::{Crust, Input, Pane, Popup};
use synth::{Player, SR};

const NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

// Colours (256-palette).
const C_LABEL: u16 = 244;
const C_CLABEL: u16 = 109; // C rows, brighter
const C_BARLINE: u16 = 240;
const C_BEAT: u16 = 238;
const C_CUR_BG: u16 = 240;
const C_CUR_FG: u16 = 231;
const C_PLAY: u16 = 51; // playhead (bright cyan)
// Note colour by strength (velocity), soft/cool -> loud/hot. 9 steps = keys 1-9.
const VEL_COLORS: [u16; 9] = [24, 31, 37, 43, 78, 148, 214, 208, 196];

const DEF_VEL: f32 = 0.8;

fn vel_color(vel: f32) -> u16 {
    let lvl = ((vel * 9.0).round() as i32).clamp(1, 9) as usize;
    VEL_COLORS[lvl - 1]
}

const GUTTER: usize = 5; // "C#4 " + edge

fn note_name(pitch: u8) -> String {
    let n = NAMES[(pitch % 12) as usize];
    let oct = (pitch / 12) as i32 - 1;
    format!("{}{}", n, oct)
}

/// Computer-keyboard piano: lower row = one octave from `base` (the octave
/// number of the `z` key, e.g. base 4 -> z = C4 = MIDI 60).
fn key_to_pitch(c: char, base: i32) -> Option<u8> {
    let semi = match c {
        'z' => 0,
        's' => 1,
        'x' => 2,
        'd' => 3,
        'c' => 4,
        'v' => 5,
        'g' => 6,
        'b' => 7,
        'h' => 8,
        'n' => 9,
        'j' => 10,
        'm' => 11,
        ',' => 12,
        _ => return None,
    };
    let midi = (base + 1) * 12 + semi;
    if (0..=127).contains(&midi) {
        Some(midi as u8)
    } else {
        None
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Edit,
    Play,
    Record,
}

struct Note {
    pitch: u8,
    start: u32, // ticks
    len: u32,   // ticks
    vel: f32,   // strength 0..1
}

struct Song {
    notes: Vec<Note>,
    bpm: u32,
    bpb: u32,       // beats per bar
    beat_unit: u32, // time-sig denominator (display only)
    tpb: u32,       // ticks per beat
    step: u32,      // grid step in ticks
    file: Option<String>,
    dirty: bool,
}

impl Song {
    fn new() -> Self {
        Song {
            notes: Vec::new(),
            bpm: 120,
            bpb: 4,
            beat_unit: 4,
            tpb: 4,
            step: 2, // 1/8 grid
            file: None,
            dirty: false,
        }
    }

    fn bar_ticks(&self) -> u32 {
        self.bpb * self.tpb
    }

    fn samples_per_tick(&self) -> f32 {
        (SR as f32) * 60.0 / (self.bpm as f32 * self.tpb as f32)
    }

    fn quantize(&self, tick: u32) -> u32 {
        ((tick + self.step / 2) / self.step) * self.step
    }

    fn grid_label(&self) -> String {
        format!("1/{}", self.beat_unit * self.tpb / self.step)
    }

    fn end_tick(&self) -> u32 {
        self.notes.iter().map(|n| n.start + n.len).max().unwrap_or(0)
    }

    /// Render the whole song to PCM. With `metro`, beat clicks are mixed in.
    fn render_pcm(&self, metro: bool) -> Vec<i16> {
        let spt = self.samples_per_tick();
        let end = self.end_tick().max(if metro { self.bar_ticks() } else { 0 });
        let total = (end as f32 * spt) as usize + (SR as f32 * 0.5) as usize + 1;
        let mut buf = vec![0f32; total.max(1)];
        for n in &self.notes {
            let at = (n.start as f32 * spt) as usize;
            let dur = (n.len as f32 * spt) as usize;
            synth::add_note(&mut buf, at, n.pitch, dur, n.vel);
        }
        if metro {
            let mut t = 0u32;
            while ((t as f32 * spt) as usize) < buf.len() {
                synth::add_click(&mut buf, (t as f32 * spt) as usize, t % self.bar_ticks() == 0);
                t += self.tpb;
            }
        }
        synth::to_pcm(&buf)
    }

    /// Parse a `.mel` text file. Unknown lines are ignored.
    fn parse(text: &str) -> Song {
        let mut song = Song::new();
        song.notes.clear();
        for line in text.lines() {
            let f: Vec<&str> = line.split_whitespace().collect();
            match f.as_slice() {
                ["bpm", v] => song.bpm = v.parse().unwrap_or(120),
                ["sig", a, b] => {
                    song.bpb = a.parse().unwrap_or(4);
                    song.beat_unit = b.parse().unwrap_or(4);
                }
                ["tpb", v] => song.tpb = v.parse().unwrap_or(4),
                ["step", v] => song.step = v.parse().unwrap_or(2),
                ["note", p, s, l] => song.notes.push(Note {
                    pitch: p.parse().unwrap_or(60),
                    start: s.parse().unwrap_or(0),
                    len: l.parse().unwrap_or(song.step),
                    vel: DEF_VEL,
                }),
                ["note", p, s, l, v] => song.notes.push(Note {
                    pitch: p.parse().unwrap_or(60),
                    start: s.parse().unwrap_or(0),
                    len: l.parse().unwrap_or(song.step),
                    vel: v.parse().unwrap_or(DEF_VEL),
                }),
                _ => {}
            }
        }
        song
    }

    /// A click-only track: `count_in` bars then `bars` bars of beats.
    fn click_track(&self, count_in: u32, bars: u32) -> Vec<i16> {
        let spt = self.samples_per_tick();
        let total_ticks = (count_in + bars) * self.bar_ticks();
        let total = (total_ticks as f32 * spt) as usize + 1;
        let mut buf = vec![0f32; total];
        let mut t = 0u32;
        while t < total_ticks {
            synth::add_click(&mut buf, (t as f32 * spt) as usize, t % self.bar_ticks() == 0);
            t += self.tpb;
        }
        synth::to_pcm(&buf)
    }
}

struct App {
    song: Song,
    mode: Mode,
    cur_tick: u32,
    cur_pitch: u8,
    top_pitch: u8,
    left_tick: u32,
    base_oct: i32,
    metro: bool,
    playhead: Option<u32>, // current playback position (ticks) while auditioning
    player: Player,
    header: Pane,
    roll: Pane,
    status: Pane,
    msg: Option<String>,
}

impl App {
    fn new() -> Self {
        let mut app = App {
            song: Song::new(),
            mode: Mode::Edit,
            cur_tick: 0,
            cur_pitch: 60,
            top_pitch: 72,
            left_tick: 0,
            base_oct: 4,
            metro: false,
            playhead: None,
            player: Player::new(),
            header: Pane::new(1, 1, 80, 1, 231, 17),
            roll: Pane::new(1, 2, 80, 20, 250, 234),
            status: Pane::new(1, 24, 80, 1, 250, 236),
            msg: None,
        };
        app.layout();
        app
    }

    fn layout(&mut self) {
        let (cols, rows) = Crust::terminal_size();
        let rows = rows.max(5);
        self.header = Pane::new(1, 1, cols, 1, 231, 17);
        self.roll = Pane::new(1, 2, cols, rows - 2, 250, 234);
        self.status = Pane::new(1, rows, cols, 1, 250, 236);
        self.header.scroll = false;
        self.roll.scroll = false;
        self.status.scroll = false;
        self.header.wrap = false;
        self.roll.wrap = false;
        self.status.wrap = false;
        Crust::clear_screen();
    }

    fn home(&self) -> String {
        std::env::var("HOME").unwrap_or_else(|_| ".".into())
    }

    fn expand(&self, p: &str) -> String {
        if let Some(r) = p.strip_prefix("~/") {
            format!("{}/{}", self.home(), r)
        } else {
            p.to_string()
        }
    }

    fn render(&mut self) {
        // --- header ---
        let modestr = match self.mode {
            Mode::Edit => "EDIT",
            Mode::Play => "PLAY",
            Mode::Record => "REC",
        };
        let fname = self
            .song
            .file
            .as_deref()
            .map(|f| f.rsplit('/').next().unwrap_or(f))
            .unwrap_or("(unsaved)");
        let dirty = if self.song.dirty { "*" } else { "" };
        let metro = if self.metro { "ON" } else { "off" };
        let h = format!(
            " melody  {} BPM  {}/{}  grid {}  metro {}  \x1b[1m[{}]\x1b[0m  oct{}  {}{}",
            self.song.bpm,
            self.song.bpb,
            self.song.beat_unit,
            self.song.grid_label(),
            metro,
            modestr,
            self.base_oct,
            fname,
            dirty
        );
        self.header.set_text(&h);
        self.header.refresh();

        // --- roll ---
        let vis_rows = self.roll.h as usize;
        let vis_ticks = (self.roll.w as usize).saturating_sub(GUTTER) as u32;

        // vertical scroll: keep cursor pitch in view
        if self.cur_pitch > self.top_pitch {
            self.top_pitch = self.cur_pitch;
        }
        let bottom = self.top_pitch as i32 - (vis_rows as i32 - 1);
        if (self.cur_pitch as i32) < bottom {
            self.top_pitch = self.cur_pitch.saturating_add(vis_rows as u8 - 1);
        }
        self.top_pitch = self.top_pitch.min(127);

        // horizontal scroll: follow the playhead while playing, else the cursor
        let anchor = self.playhead.unwrap_or(self.cur_tick);
        if anchor < self.left_tick {
            self.left_tick = anchor;
        }
        if anchor >= self.left_tick + vis_ticks {
            self.left_tick = anchor - vis_ticks + 1;
        }
        let bar = self.song.bar_ticks();
        self.left_tick = (self.left_tick / bar) * bar;

        let mut lines: Vec<String> = Vec::with_capacity(vis_rows);
        for i in 0..vis_rows {
            if (self.top_pitch as usize) < i {
                lines.push(String::new());
                continue;
            }
            let p = self.top_pitch - i as u8;
            let lcol = if p % 12 == 0 { C_CLABEL } else { C_LABEL };
            let mut line = format!("\x1b[38;5;{}m{:>4}\x1b[0m ", lcol, note_name(p));
            for c in 0..vis_ticks {
                let t = self.left_tick + c;
                let note_vel = self
                    .song
                    .notes
                    .iter()
                    .find(|n| n.pitch == p && t >= n.start && t < n.start + n.len)
                    .map(|n| n.vel);
                let is_play = self.playhead == Some(t);
                let is_cur = p == self.cur_pitch && t == self.cur_tick;
                if is_play {
                    // moving playhead: light up the column; notes on it glow
                    let ch = if note_vel.is_some() { '\u{2588}' } else { '\u{2502}' };
                    line.push_str(&format!("\x1b[38;5;{}m{}\x1b[0m", C_PLAY, ch));
                } else if is_cur {
                    let ch = if note_vel.is_some() { '\u{2588}' } else { '\u{258F}' };
                    line.push_str(&format!(
                        "\x1b[48;5;{}m\x1b[38;5;{}m{}\x1b[0m",
                        C_CUR_BG, C_CUR_FG, ch
                    ));
                } else if let Some(v) = note_vel {
                    line.push_str(&format!("\x1b[38;5;{}m\u{2588}\x1b[0m", vel_color(v)));
                } else if t % bar == 0 {
                    line.push_str(&format!("\x1b[38;5;{}m\u{2502}\x1b[0m", C_BARLINE));
                } else if t % self.song.tpb == 0 {
                    line.push_str(&format!("\x1b[38;5;{}m\u{00b7}\x1b[0m", C_BEAT));
                } else {
                    line.push(' ');
                }
            }
            lines.push(line);
        }
        self.roll.set_text(&lines.join("\n"));
        self.roll.refresh();

        // --- status: transient message, else mode help ---
        let status = if let Some(m) = self.msg.take() {
            m
        } else {
            match self.mode {
                Mode::Edit => " hjkl move · Enter add · HJKL move-note · +/- len · 1-9 strength · x del · Spc play · r REC · Tab jam · m metro · e export · ? help · q quit".to_string(),
                Mode::Play => " JAM — z s x d c v g b h n j m , = piano · [ ] octave · Esc/Tab back to edit".to_string(),
                Mode::Record => " \u{25cf} REC — play along · Esc/Enter to stop".to_string(),
            }
        };
        self.status.set_text(&status);
        self.status.refresh();
    }

    /// Index of the note under the cursor, if any.
    fn sel(&self) -> Option<usize> {
        self.song.notes.iter().position(|n| {
            n.pitch == self.cur_pitch && self.cur_tick >= n.start && self.cur_tick < n.start + n.len
        })
    }

    /// Play a single note for live feedback.
    fn monitor(&mut self, pitch: u8, vel: f32) {
        let mut buf = vec![0f32; (SR as f32 * 0.45) as usize];
        synth::add_note(&mut buf, 0, pitch, (SR as f32 * 0.05) as usize, vel);
        self.player.note(synth::to_pcm(&buf));
    }

    /// Modal keybinding help (`?`). Dismiss with Esc/q/Enter.
    fn help(&mut self) {
        let content = "\
 melody — keys

 EDIT
  h j k l / arrows   move cursor (time / pitch)
  Enter              add note at cursor / hear it
  H  L               move note earlier / later
  K  J               move note up / down a semitone
  + / -              lengthen / shorten note
  1 .. 9             note strength (soft .. loud)
  x / Del            delete note
  Space              play / stop the melody
  m                  metronome on / off
  g                  grid:  1/4  1/8  1/16
  , .                tempo  -/+ 5 BPM
  r                  record       Tab   jam (free play)
  w  o  e            save / open / export WAV
  n                  new          q     quit

 PLAY / REC  (keyboard piano)
  z s x d c v g b h n j m ,   = C C# D .. C
  [ ]                octave down / up
  Esc / Enter        stop / back to edit

 Strength also sets each note's colour (cool=soft, hot=loud).

 (Esc / q / Enter to close)";
        let (cols, rows) = Crust::terminal_size();
        let lines = content.lines().count() as u16;
        let w = 62.min(cols.saturating_sub(2)).max(20);
        let h = lines.min(rows.saturating_sub(2)).max(3);
        let mut pop = Popup::centered(w, h, C_CUR_FG, 17);
        pop.modal(content);
        pop.dismiss(&mut [&mut self.header, &mut self.roll, &mut self.status]);
        self.render();
    }

    fn handle_edit(&mut self, key: &str) -> bool {
        let step = self.song.step;
        match key {
            "q" => {
                if self.song.dirty {
                    let a = self.status.ask(" Quit without saving? (y/N): ", "");
                    if !a.eq_ignore_ascii_case("y") {
                        return true;
                    }
                }
                return false;
            }
            "h" | "LEFT" => self.cur_tick = self.cur_tick.saturating_sub(step),
            "l" | "RIGHT" => self.cur_tick += step,
            "k" | "UP" => self.cur_pitch = (self.cur_pitch + 1).min(127),
            "j" | "DOWN" => self.cur_pitch = self.cur_pitch.saturating_sub(1),
            // Move the selected note (capitals are reliable across terminals).
            "L" => {
                if let Some(i) = self.sel() {
                    self.song.notes[i].start += step;
                    self.cur_tick += step;
                    self.song.dirty = true;
                }
            }
            "H" => {
                if let Some(i) = self.sel() {
                    let s = self.song.notes[i].start;
                    let ns = s.saturating_sub(step);
                    self.song.notes[i].start = ns;
                    self.cur_tick = self.cur_tick.saturating_sub(s - ns);
                    self.song.dirty = true;
                }
            }
            "K" => {
                if let Some(i) = self.sel() {
                    if self.song.notes[i].pitch < 127 {
                        self.song.notes[i].pitch += 1;
                        self.cur_pitch = self.song.notes[i].pitch;
                        self.song.dirty = true;
                    }
                }
            }
            "J" => {
                if let Some(i) = self.sel() {
                    if self.song.notes[i].pitch > 0 {
                        self.song.notes[i].pitch -= 1;
                        self.cur_pitch = self.song.notes[i].pitch;
                        self.song.dirty = true;
                    }
                }
            }
            "+" | "=" => {
                if let Some(i) = self.sel() {
                    let n = &mut self.song.notes[i];
                    // Step by the grid above one step; by single ticks below it,
                    // so a note can grow past or shrink under the default length.
                    n.len = if n.len >= step { n.len + step } else { n.len + 1 };
                    self.song.dirty = true;
                }
            }
            "-" => {
                if let Some(i) = self.sel() {
                    let n = &mut self.song.notes[i];
                    n.len = if n.len > step {
                        n.len - step
                    } else {
                        n.len.saturating_sub(1).max(1)
                    };
                    self.song.dirty = true;
                }
            }
            "x" | "DEL" => {
                if let Some(i) = self.sel() {
                    self.song.notes.remove(i);
                    self.song.dirty = true;
                }
            }
            "ENTER" => {
                let p = self.cur_pitch;
                let vel = match self.sel() {
                    Some(i) => self.song.notes[i].vel,
                    None => {
                        self.song.notes.push(Note {
                            pitch: p,
                            start: self.cur_tick,
                            len: step,
                            vel: DEF_VEL,
                        });
                        self.song.dirty = true;
                        DEF_VEL
                    }
                };
                self.monitor(p, vel);
            }
            // Digit 1-9 sets the strength (velocity) of the selected note.
            d if d.len() == 1 && d.chars().next().unwrap().is_ascii_digit() && d != "0" => {
                if let Some(i) = self.sel() {
                    let lvl = d.parse::<u8>().unwrap() as f32;
                    self.song.notes[i].vel = lvl / 9.0;
                    self.song.dirty = true;
                    let (p, v) = (self.song.notes[i].pitch, self.song.notes[i].vel);
                    self.monitor(p, v);
                }
            }
            " " => self.play_loop(),
            "TAB" => self.mode = Mode::Play,
            "r" => self.mode = Mode::Record,
            "m" => self.metro = !self.metro,
            "g" => {
                // cycle 1/4 -> 1/8 -> 1/16
                self.song.step = match self.song.step {
                    s if s == self.song.tpb => self.song.tpb / 2,
                    s if s == self.song.tpb / 2 => (self.song.tpb / 4).max(1),
                    _ => self.song.tpb,
                };
            }
            "," => self.song.bpm = self.song.bpm.saturating_sub(5).max(20),
            "." => self.song.bpm = (self.song.bpm + 5).min(300),
            "w" => self.save(),
            "o" => self.open(),
            "e" => self.export(),
            "n" => {
                let a = self.status.ask(" New song, clear all? (y/N): ", "");
                if a.eq_ignore_ascii_case("y") {
                    self.song.notes.clear();
                    self.song.dirty = false;
                    self.song.file = None;
                }
            }
            _ => {}
        }
        true
    }

    fn handle_play(&mut self, key: &str) {
        match key {
            "ESC" | "TAB" | "ENTER" => self.mode = Mode::Edit,
            "[" => self.base_oct = (self.base_oct - 1).max(-1),
            "]" => self.base_oct = (self.base_oct + 1).min(9),
            k if k.chars().count() == 1 => {
                if let Some(p) = key_to_pitch(k.chars().next().unwrap(), self.base_oct) {
                    self.monitor(p, DEF_VEL);
                }
            }
            _ => {}
        }
    }

    /// Real-time capture. Plays a metronome click track (when enabled) and
    /// records quantized notes against it until Esc/Enter or the cap.
    fn record_loop(&mut self) {
        use crossterm::event::{self, Event, KeyCode, KeyEventKind};
        use std::time::{Duration, Instant};

        let spt = self.song.samples_per_tick();
        let count_in = if self.metro { 1 } else { 0 };
        let cap_bars = 64u32;
        if self.metro {
            let track = self.song.click_track(count_in, cap_bars);
            self.player.play(track);
        }
        let start = Instant::now();
        let count_in_secs =
            count_in as f32 * self.song.bar_ticks() as f32 * spt / SR as f32;
        let origin = start + Duration::from_secs_f32(count_in_secs);
        let cap_secs = (count_in + cap_bars) as f32 * self.song.bar_ticks() as f32 * spt
            / SR as f32
            + 1.0;
        self.render();

        loop {
            if event::poll(Duration::from_millis(8)).unwrap_or(false) {
                if let Ok(Event::Key(k)) = event::read() {
                    if k.kind != KeyEventKind::Press {
                        continue;
                    }
                    match k.code {
                        KeyCode::Esc | KeyCode::Enter => break,
                        KeyCode::Char('[') => self.base_oct = (self.base_oct - 1).max(-1),
                        KeyCode::Char(']') => self.base_oct = (self.base_oct + 1).min(9),
                        KeyCode::Char(c) => {
                            if let Some(p) = key_to_pitch(c, self.base_oct) {
                                self.monitor(p, DEF_VEL);
                                let now = Instant::now();
                                if now >= origin {
                                    let secs = (now - origin).as_secs_f32();
                                    let tick = (secs * SR as f32 / spt) as u32;
                                    let qt = self.song.quantize(tick);
                                    self.song.notes.push(Note {
                                        pitch: p,
                                        start: qt,
                                        len: self.song.step,
                                        vel: DEF_VEL,
                                    });
                                    self.song.dirty = true;
                                    self.render();
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            if start.elapsed().as_secs_f32() > cap_secs {
                break;
            }
        }
        self.player.stop_all();
        self.song.notes.sort_by_key(|n| (n.start, n.pitch));
        self.mode = Mode::Edit;
    }

    /// Audition the melody with a moving playhead. Any key stops it early;
    /// otherwise it runs to the last note and lets the tail ring out.
    fn play_loop(&mut self) {
        use crossterm::event::{self, Event, KeyEventKind};
        use std::time::{Duration, Instant};

        let end = self.song.end_tick();
        if end == 0 {
            return;
        }
        let spt = self.song.samples_per_tick();
        let pcm = self.song.render_pcm(self.metro);
        self.player.play(pcm);
        let start = Instant::now();
        let total_secs = end as f32 * spt / SR as f32 + 0.25;

        let mut keyed = false;
        loop {
            let elapsed = start.elapsed().as_secs_f32();
            if elapsed > total_secs {
                break;
            }
            self.playhead = Some((elapsed * SR as f32 / spt) as u32);
            self.render();
            if event::poll(Duration::from_millis(40)).unwrap_or(false) {
                if let Ok(Event::Key(k)) = event::read() {
                    if k.kind == KeyEventKind::Press {
                        keyed = true;
                        break;
                    }
                }
            }
        }
        self.playhead = None;
        if keyed {
            self.player.stop(); // hard-stop on key; natural end lets it ring
        }
        self.render();
    }

    fn save(&mut self) {
        let def = self
            .song
            .file
            .clone()
            .unwrap_or_else(|| format!("{}/.melody/untitled.mel", self.home()));
        let path = self.status.ask(" Save to: ", &def);
        if self.status.last_escaped || path.is_empty() {
            return;
        }
        let path = self.expand(&path);
        let mut s = String::new();
        s.push_str("melody 1\n");
        s.push_str(&format!("bpm {}\n", self.song.bpm));
        s.push_str(&format!("sig {} {}\n", self.song.bpb, self.song.beat_unit));
        s.push_str(&format!("tpb {}\n", self.song.tpb));
        s.push_str(&format!("step {}\n", self.song.step));
        for n in &self.song.notes {
            s.push_str(&format!("note {} {} {} {:.2}\n", n.pitch, n.start, n.len, n.vel));
        }
        match std::fs::write(&path, s) {
            Ok(_) => {
                self.song.file = Some(path.clone());
                self.song.dirty = false;
                self.msg = Some(format!(" Saved {}", path));
            }
            Err(e) => self.msg = Some(format!(" ERROR saving: {}", e)),
        }
    }

    fn open(&mut self) {
        let path = self.status.ask(" Open: ", &format!("{}/.melody/", self.home()));
        if self.status.last_escaped || path.is_empty() {
            return;
        }
        let path = self.expand(&path);
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                self.msg = Some(format!(" ERROR opening: {}", e));
                return;
            }
        };
        let mut song = Song::parse(&text);
        song.file = Some(path.clone());
        self.song = song;
        self.cur_tick = 0;
        self.cur_pitch = 60;
        self.msg = Some(format!(" Opened {}", path));
    }

    fn export(&mut self) {
        let def = match &self.song.file {
            Some(f) if f.ends_with(".mel") => format!("{}wav", &f[..f.len() - 3]),
            _ => format!("{}/.melody/melody.wav", self.home()),
        };
        let path = self.status.ask(" Export WAV to: ", &def);
        if self.status.last_escaped || path.is_empty() {
            return;
        }
        let path = self.expand(&path);
        let pcm = self.song.render_pcm(false);
        match synth::write_wav(&path, &pcm) {
            Ok(_) => self.msg = Some(format!(" Exported {} ({} notes)", path, self.song.notes.len())),
            Err(e) => self.msg = Some(format!(" ERROR exporting: {}", e)),
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // Headless: melody --render in.mel out.wav  (no TUI, for batch export).
    if args.get(1).map(|s| s.as_str()) == Some("--render") {
        let (inp, outp) = match (args.get(2), args.get(3)) {
            (Some(i), Some(o)) => (i.clone(), o.clone()),
            _ => {
                eprintln!("usage: melody --render <in.mel> <out.wav>");
                std::process::exit(1);
            }
        };
        let text = match std::fs::read_to_string(&inp) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("melody: cannot read {}: {}", inp, e);
                std::process::exit(1);
            }
        };
        let song = Song::parse(&text);
        let pcm = song.render_pcm(false);
        match synth::write_wav(&outp, &pcm) {
            Ok(_) => {
                println!("melody: wrote {} ({} notes)", outp, song.notes.len());
                std::process::exit(0);
            }
            Err(e) => {
                eprintln!("melody: cannot write {}: {}", outp, e);
                std::process::exit(1);
            }
        }
    }

    // Optional: melody <file.mel> opens it.
    let arg = std::env::args().nth(1);
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    let _ = std::fs::create_dir_all(format!("{}/.melody", home));

    Crust::init();
    Crust::set_app_identity("Melody");

    let mut app = App::new();
    if let Some(a) = arg {
        let path = app.expand(&a);
        if let Ok(text) = std::fs::read_to_string(&path) {
            let mut song = Song::parse(&text);
            song.file = Some(path);
            app.song = song;
        }
    }
    if app.player.silent() {
        app.msg = Some(" no audio player found (install alsa-utils or sox) — export still works".into());
    }
    app.render();

    loop {
        if app.mode == Mode::Record {
            app.record_loop();
            app.render();
            continue;
        }
        let key = Input::getchr(None).unwrap_or_default();
        if key == "RESIZE" {
            app.layout();
            app.render();
            continue;
        }
        if key == "?" {
            app.help();
            continue;
        }
        match app.mode {
            Mode::Edit => {
                if !app.handle_edit(&key) {
                    break;
                }
            }
            Mode::Play => app.handle_play(&key),
            Mode::Record => {}
        }
        app.render();
        app.player.reap();
    }

    app.player.stop_all();
    Crust::cleanup();
}
