//! Graphical orb UI for the voice assistant
//!
//! Provides a visual representation of the assistant's state using animated
//! ASCII art orbs. Supports two visual styles: Rings and Blob.

use crate::render::{OrbStyle, UiEvent, UiMode, UiRenderer};
use crate::state::AppMode;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::style::Color;
use crossterm::terminal::{self, ClearType};
use crossterm::{cursor, execute};
use std::io::{self, Write, stdout};
use std::time::Instant;

const TAU: f64 = std::f64::consts::TAU;

// ============================================================================
// Orb State (maps to assistant states)
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OrbState {
    Idle,
    Listening,
    Thinking,
    Speaking,
}

impl OrbState {
    fn frequency(&self) -> f64 {
        match self {
            OrbState::Idle => 0.35,
            OrbState::Listening => 0.9,
            OrbState::Thinking => 1.4,
            OrbState::Speaking => 1.0,
        }
    }

    fn palette(&self) -> Palette {
        match self {
            OrbState::Idle => Palette {
                core: hsl(210.0, 0.7, 0.75),
                mid: hsl(220.0, 0.8, 0.55),
                edge: hsl(230.0, 0.9, 0.35),
                glow: hsl(200.0, 0.6, 0.25),
            },
            OrbState::Listening => Palette {
                core: hsl(160.0, 0.9, 0.7),
                mid: hsl(170.0, 0.85, 0.5),
                edge: hsl(180.0, 0.8, 0.35),
                glow: hsl(165.0, 0.7, 0.2),
            },
            OrbState::Thinking => Palette {
                core: hsl(280.0, 0.8, 0.75),
                mid: hsl(270.0, 0.85, 0.55),
                edge: hsl(260.0, 0.9, 0.4),
                glow: hsl(275.0, 0.7, 0.25),
            },
            OrbState::Speaking => Palette {
                core: hsl(40.0, 1.0, 0.7),
                mid: hsl(30.0, 1.0, 0.55),
                edge: hsl(20.0, 0.95, 0.4),
                glow: hsl(25.0, 0.8, 0.25),
            },
        }
    }
}

// ============================================================================
// Color utilities
// ============================================================================

#[derive(Clone, Copy)]
struct Rgb(f64, f64, f64);

impl Rgb {
    fn lerp(self, other: Rgb, t: f64) -> Rgb {
        Rgb(
            self.0 + (other.0 - self.0) * t,
            self.1 + (other.1 - self.1) * t,
            self.2 + (other.2 - self.2) * t,
        )
    }

    fn scale(self, s: f64) -> Rgb {
        Rgb(self.0 * s, self.1 * s, self.2 * s)
    }

    fn add(self, other: Rgb) -> Rgb {
        Rgb(self.0 + other.0, self.1 + other.1, self.2 + other.2)
    }

    fn to_terminal(self) -> Color {
        Color::Rgb {
            r: (self.0.clamp(0.0, 1.0) * 255.0) as u8,
            g: (self.1.clamp(0.0, 1.0) * 255.0) as u8,
            b: (self.2.clamp(0.0, 1.0) * 255.0) as u8,
        }
    }
}

fn hsl(h: f64, s: f64, l: f64) -> Rgb {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let h = h / 60.0;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    Rgb(r + m, g + m, b + m)
}

#[derive(Clone, Copy)]
struct Palette {
    core: Rgb,
    mid: Rgb,
    edge: Rgb,
    glow: Rgb,
}

impl Palette {
    fn lerp(self, other: Palette, t: f64) -> Palette {
        Palette {
            core: self.core.lerp(other.core, t),
            mid: self.mid.lerp(other.mid, t),
            edge: self.edge.lerp(other.edge, t),
            glow: self.glow.lerp(other.glow, t),
        }
    }

    fn sample(&self, t: f64) -> Rgb {
        if t < 0.3 {
            self.core.lerp(self.mid, t / 0.3)
        } else if t < 0.7 {
            self.mid.lerp(self.edge, (t - 0.3) / 0.4)
        } else {
            self.edge.lerp(self.glow, (t - 0.7) / 0.3)
        }
    }
}

// ============================================================================
// Noise functions for organic movement
// ============================================================================

fn hash(x: f64, y: f64, z: f64) -> f64 {
    let n = (x * 127.1 + y * 311.7 + z * 74.7).sin() * 43758.5453;
    n.fract()
}

fn smooth_noise(x: f64, y: f64, z: f64) -> f64 {
    let xi = x.floor();
    let yi = y.floor();
    let zi = z.floor();
    let xf = x - xi;
    let yf = y - yi;
    let zf = z - zi;

    let u = xf * xf * (3.0 - 2.0 * xf);
    let v = yf * yf * (3.0 - 2.0 * yf);
    let w = zf * zf * (3.0 - 2.0 * zf);

    let c000 = hash(xi, yi, zi);
    let c100 = hash(xi + 1.0, yi, zi);
    let c010 = hash(xi, yi + 1.0, zi);
    let c110 = hash(xi + 1.0, yi + 1.0, zi);
    let c001 = hash(xi, yi, zi + 1.0);
    let c101 = hash(xi + 1.0, yi, zi + 1.0);
    let c011 = hash(xi, yi + 1.0, zi + 1.0);
    let c111 = hash(xi + 1.0, yi + 1.0, zi + 1.0);

    let x00 = c000 + (c100 - c000) * u;
    let x10 = c010 + (c110 - c010) * u;
    let x01 = c001 + (c101 - c001) * u;
    let x11 = c011 + (c111 - c011) * u;

    let y0 = x00 + (x10 - x00) * v;
    let y1 = x01 + (x11 - x01) * v;

    y0 + (y1 - y0) * w
}

fn fbm(x: f64, y: f64, z: f64, octaves: usize, persistence: f64) -> f64 {
    let mut value = 0.0;
    let mut amplitude = 0.5;
    let mut frequency = 1.0;
    let mut max_value = 0.0;

    for _ in 0..octaves {
        value += amplitude * smooth_noise(x * frequency, y * frequency, z * frequency);
        max_value += amplitude;
        amplitude *= persistence;
        frequency *= 2.0;
    }
    value / max_value
}

fn ease_out_quart(t: f64) -> f64 {
    1.0 - (1.0 - t).powi(4)
}

// ============================================================================
// Orb renderer core
// ============================================================================

struct Orb {
    state: OrbState,
    target_state: OrbState,
    time: f64,
    transition: f64,
    audio_level: f64,
    audio_freqs: [f64; 8],
    smooth_audio: f64,
    smooth_freqs: [f64; 8],
    style: OrbStyle,
    secondary_audio: f64,
    smooth_secondary: f64,
}

impl Orb {
    fn new(style: OrbStyle) -> Self {
        Self {
            state: OrbState::Idle,
            target_state: OrbState::Idle,
            time: 0.0,
            transition: 1.0,
            audio_level: 0.0,
            audio_freqs: [0.0; 8],
            smooth_audio: 0.0,
            smooth_freqs: [0.0; 8],
            style,
            secondary_audio: 0.0,
            smooth_secondary: 0.0,
        }
    }

    fn set_state(&mut self, state: OrbState) {
        if state != self.target_state {
            self.state = self.target_state;
            self.target_state = state;
            self.transition = 0.0;
        }
    }

    fn set_style(&mut self, style: OrbStyle) {
        self.style = style;
    }

    fn set_audio(&mut self, level: f64) {
        self.audio_level = level.clamp(0.0, 1.0);
        // Generate frequency bands from audio level with some variation
        for i in 0..8 {
            let phase = self.time * (0.5 + i as f64 * 0.15);
            self.audio_freqs[i] = (level * (0.5 + 0.5 * (phase).sin())).clamp(0.0, 1.0);
        }
    }

    fn set_secondary_audio(&mut self, level: f64) {
        self.secondary_audio = level.clamp(0.0, 1.0);
    }

    fn update(&mut self, dt: f64) {
        self.time += dt;
        self.transition = (self.transition + dt * 2.0).min(1.0);

        let k = 1.0 - (-dt * 15.0).exp();
        self.smooth_audio += (self.audio_level - self.smooth_audio) * k;
        self.smooth_secondary += (self.secondary_audio - self.smooth_secondary) * k;
        for i in 0..8 {
            self.smooth_freqs[i] += (self.audio_freqs[i] - self.smooth_freqs[i]) * k;
        }
    }

    fn current_palette(&self) -> Palette {
        let t = ease_out_quart(self.transition);
        self.state.palette().lerp(self.target_state.palette(), t)
    }

    fn current_frequency(&self) -> f64 {
        let t = ease_out_quart(self.transition);
        self.state.frequency() + (self.target_state.frequency() - self.state.frequency()) * t
    }

    // Ring style renderer
    fn sample_rings(&self, x: f64, y: f64, max_r: f64) -> (f64, f64) {
        let freq = self.current_frequency();
        let x_squash = 0.4;
        let x_scaled = x * x_squash;

        let wave_freq = 2.0;
        let wave_speed = 0.4;
        let wave_amp = max_r * 0.025 * (1.0 + self.smooth_audio * 0.3);

        let x_norm = x / max_r;
        let y_wave = (x_norm * wave_freq + self.time * freq * TAU * wave_speed).sin() * wave_amp;
        let y_displaced = y + y_wave;

        let dist = (x_scaled * x_scaled + y_displaced * y_displaced).sqrt();
        let angle = y_displaced.atan2(x_scaled);
        let r = dist / max_r;

        if r > 0.7 {
            return (0.0, 0.0);
        }

        let mut intensity = 0.0;
        let mut glow = 0.0;

        // Central core
        let core_dist = ((x * x_squash).powi(2) + (y * 0.8).powi(2)).sqrt() / max_r;
        let core_pulse = 1.0 + 0.15 * (self.time * freq * TAU).sin();
        let core_size = (0.04 + self.smooth_audio * 0.02) * core_pulse;
        let core = (-(core_dist * core_dist) / (2.0 * core_size * core_size)).exp();
        intensity += core * 0.9;
        glow += core * 1.2;

        // Concentric rings
        let ring_count = match self.target_state {
            OrbState::Idle => 3,
            OrbState::Listening => 4,
            OrbState::Thinking => 5,
            OrbState::Speaking => 4,
        };

        let inner_r = 0.20;
        let outer_r = 0.38;

        for i in 0..ring_count {
            let ring_phase = i as f64 / (ring_count - 1).max(1) as f64;
            let base_r = inner_r + ring_phase * (outer_r - inner_r);

            let breath_phase = self.time * freq * TAU + ring_phase * TAU * 1.5;
            let breath = breath_phase.sin() * 0.012;

            let band = (i * 8 / ring_count).min(7);
            let audio_r = self.smooth_freqs[band] * 0.025;

            let wobble = self.ring_wobble(angle, i, self.time * freq);
            let ring_r = base_r + breath + audio_r + wobble;

            let width = 0.010 + self.smooth_audio * 0.003;
            let d = (r - ring_r).abs();
            let ring_intensity = (-d * d / (2.0 * width * width)).exp();

            let fade = 1.0 - ring_phase * 0.25;
            let edge_y = (y / max_r).abs();
            let edge_bright = 0.8 + edge_y.powf(0.5) * 0.2;

            intensity += ring_intensity * fade * edge_bright * 0.55;
            glow += ring_intensity * fade * 0.2;
        }

        let ambient = (-(r - outer_r).max(0.0).powi(2) * 25.0).exp() * 0.05;
        glow += ambient;

        (intensity.min(1.0), glow.min(1.0))
    }

    fn ring_wobble(&self, angle: f64, ring_idx: usize, t: f64) -> f64 {
        let mut w = 0.0;
        for h in 1..=4 {
            let hf = h as f64;
            let speed = 0.3 + (ring_idx as f64 * 0.07);
            let phase = t * hf * speed + ring_idx as f64 * 0.6;
            w += (angle * hf * 2.0 + phase * TAU).sin() * 0.008 / hf;
        }
        let band = ((angle / TAU + 0.5).fract() * 8.0) as usize;
        w += self.smooth_freqs[band] * 0.015;
        w
    }

    // Blob style renderer
    fn sample_blob(&self, x: f64, y: f64, max_r: f64) -> (f64, f64) {
        let dist = (x * x + y * y).sqrt();
        let angle = y.atan2(x);
        let r = dist / max_r;

        if r > 1.4 {
            return (0.0, 0.0);
        }

        let freq = self.current_frequency();
        let t = self.time * freq;

        let noise_scale = match self.target_state {
            OrbState::Idle => 1.5,
            OrbState::Listening => 2.0,
            OrbState::Thinking => 3.0,
            OrbState::Speaking => 2.2,
        };

        let octaves = match self.target_state {
            OrbState::Idle => 3,
            OrbState::Thinking => 5,
            _ => 4,
        };

        let nx = angle.cos() * noise_scale;
        let ny = angle.sin() * noise_scale;
        let nz = t * 0.8;

        let noise = fbm(nx, ny, nz, octaves, 0.5);

        let base_radius = 0.55 + self.smooth_audio * 0.15;
        let deform = (noise - 0.5) * 0.35;
        let mut blob_radius = base_radius + deform;

        let band = ((angle / TAU + 0.5).fract() * 8.0) as usize;
        let audio_bulge = self.smooth_freqs[band] * 0.2;
        blob_radius += audio_bulge;

        let surface_dist = r - blob_radius;

        let interior = if surface_dist < 0.0 {
            let depth = -surface_dist / blob_radius;
            let inner_noise = fbm(nx * 2.0, ny * 2.0, nz * 1.5, 3, 0.5);
            let structure = 0.3 + inner_noise * 0.4;
            let core_dist = r / blob_radius;
            let core = (-(core_dist * core_dist) * 3.0).exp();
            (depth * 0.6 + structure * 0.3 + core * 0.8).min(1.0)
        } else {
            0.0
        };

        let surface_glow = (-surface_dist.abs() * 8.0).exp() * 0.6;
        let atmo = if surface_dist > 0.0 {
            (-surface_dist * 4.0).exp() * 0.3
        } else {
            0.0
        };

        (interior.min(1.0), (surface_glow + atmo).min(1.0))
    }

    fn render(&self, width: usize, height: usize) -> Vec<Vec<(char, Color)>> {
        let mut buffer = vec![vec![(' ', Color::Reset); width]; height];
        let palette = self.current_palette();

        let aspect = 2.1;
        let max_r = (height as f64).min(width as f64 / aspect) * 0.48;
        let cx = width as f64 / 2.0;
        let cy = height as f64 / 2.0;

        let shades: &[char] = &[' ', '.', ':', '-', '=', '+', '*', '#', '@'];

        for row in 0..height {
            for col in 0..width {
                let x = (col as f64 - cx) / aspect;
                let y = row as f64 - cy;

                let (intensity, glow) = match self.style {
                    OrbStyle::Rings => self.sample_rings(x, y, max_r),
                    OrbStyle::Blob => self.sample_blob(x, y, max_r),
                };

                if intensity < 0.01 && glow < 0.02 {
                    continue;
                }

                let dist = (x * x + y * y).sqrt() / max_r;
                let color_t = (dist * 1.1).min(1.0);

                let base_color = palette.sample(color_t);
                let brightness = intensity * 0.75 + glow * 0.35;
                let mut final_color = base_color.scale(0.25 + brightness * 0.75);

                let combined = intensity;
                if combined > 0.75 {
                    let highlight = (combined - 0.75) * 2.0;
                    final_color = final_color.add(Rgb(highlight, highlight, highlight).scale(0.25));
                }

                let char_intensity = (intensity + glow * 0.25).min(1.0);
                let idx = ((char_intensity * (shades.len() - 1) as f64).round() as usize)
                    .min(shades.len() - 1);

                let ch = shades[idx];
                if ch != ' ' {
                    buffer[row][col] = (ch, final_color.to_terminal());
                }
            }
        }

        buffer
    }
}

// ============================================================================
// GraphicalUi - main UI implementation
// ============================================================================

pub struct GraphicalUi {
    orb: Orb,
    last_frame: Instant,
    // State from text UI that we also need
    status: String,
    preview: String,
    input: String,
    cursor_pos: usize,
    ready: bool,
    responding: bool,
    context_words: usize,
    last_response_words: usize,
    mic_muted: bool,
    tts_enabled: bool,
    wake_enabled: bool,
    mode: AppMode,
    input_activity: bool,
    keypress_activity: bool,
    auto_submit_progress: Option<f32>,
    audio_level: f32,
    tts_level: f32,
}

impl GraphicalUi {
    pub fn new() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        execute!(
            stdout(),
            terminal::EnterAlternateScreen,
            cursor::Hide,
            terminal::Clear(ClearType::All)
        )?;

        Ok(Self {
            orb: Orb::new(OrbStyle::Rings),
            last_frame: Instant::now(),
            status: "Loading...".to_string(),
            preview: String::new(),
            input: String::new(),
            cursor_pos: 0,
            ready: false,
            responding: false,
            context_words: 0,
            last_response_words: 0,
            mic_muted: false,
            tts_enabled: true,
            wake_enabled: true,
            mode: AppMode::Chat,
            input_activity: false,
            keypress_activity: false,
            auto_submit_progress: None,
            audio_level: 0.0,
            tts_level: 0.0,
        })
    }

    fn char_to_byte_index(&self, char_idx: usize) -> usize {
        self.input
            .char_indices()
            .nth(char_idx)
            .map(|(i, _)| i)
            .unwrap_or(self.input.len())
    }

    fn char_count(&self) -> usize {
        self.input.chars().count()
    }
}

impl UiRenderer for GraphicalUi {
    fn handle_ui_event(&mut self, event: UiEvent) -> io::Result<()> {
        match event {
            UiEvent::Preview(text) => {
                self.preview = text;
                self.status = "Listening".to_string();
                self.orb.set_state(OrbState::Listening);
            }
            UiEvent::Final(_text) => {
                self.preview.clear();
                self.status = "Processing".to_string();
            }
            UiEvent::Thinking => {
                self.status = "Thinking".to_string();
                self.orb.set_state(OrbState::Thinking);
            }
            UiEvent::Speaking => {
                self.status = "Speaking".to_string();
                self.orb.set_state(OrbState::Speaking);
            }
            UiEvent::SpeakingDone => {
                self.ready = true;
                self.status = "Ready".to_string();
                self.orb.set_state(OrbState::Idle);
            }
            UiEvent::ResponseChunk(text) => {
                self.responding = true;
                // In graphical mode, we might show response differently
                // For now, just accumulate (could show in a floating panel)
                let _ = text;
            }
            UiEvent::ResponseEnd => {
                self.responding = false;
            }
            UiEvent::Idle => {
                self.status = if self.ready {
                    "Ready".to_string()
                } else {
                    "Idle".to_string()
                };
                self.orb.set_state(OrbState::Idle);
                self.preview.clear();
            }
            UiEvent::Tick => {}
            UiEvent::ContextWords(count) => {
                self.context_words = count;
            }
        }
        Ok(())
    }

    fn draw(&mut self) -> io::Result<()> {
        let now = Instant::now();
        let dt = now.duration_since(self.last_frame).as_secs_f64();
        self.last_frame = now;

        // Update orb with audio levels
        let audio = if self.orb.target_state == OrbState::Listening {
            self.audio_level as f64
        } else if self.orb.target_state == OrbState::Speaking {
            self.tts_level as f64
        } else {
            0.1
        };
        self.orb.set_audio(audio);
        self.orb.set_secondary_audio(self.tts_level as f64);
        self.orb.update(dt);

        let (tw, th) = terminal::size()?;
        let w = tw as usize;
        let h = (th as usize).saturating_sub(3); // Reserve space for status bars

        let buf = self.orb.render(w, h);

        // Build output string
        let mut out = String::with_capacity(w * h * 24);
        out.push_str("\x1b[H"); // Home cursor

        let mut last_color: Option<(u8, u8, u8)> = None;

        for (ri, row) in buf.iter().enumerate() {
            for (ch, color) in row {
                let rgb = match color {
                    Color::Rgb { r, g, b } => (*r, *g, *b),
                    _ => (0, 0, 0),
                };

                if *ch == ' ' && rgb == (0, 0, 0) {
                    out.push(' ');
                } else {
                    if last_color != Some(rgb) {
                        out.push_str(&format!("\x1b[38;2;{};{};{}m", rgb.0, rgb.1, rgb.2));
                        last_color = Some(rgb);
                    }
                    out.push(*ch);
                }
            }
            if ri < buf.len() - 1 {
                out.push_str("\r\n");
            }
        }

        // Reset color and draw status bar
        out.push_str("\x1b[0m\r\n");

        // Status line
        let mode_str = match self.mode {
            AppMode::Chat => "\x1b[92mChat\x1b[0m",
            AppMode::Paused => "\x1b[33mPaused\x1b[0m",
            AppMode::Transcribe => "\x1b[93mTranscribe\x1b[0m",
            AppMode::NoteTaking => "\x1b[95mNote\x1b[0m",
            AppMode::Command => "\x1b[96mCommand\x1b[0m",
        };

        let toggles = format!(
            "{}{}{}",
            if self.mic_muted {
                "\x1b[31m[MIC OFF]\x1b[0m"
            } else {
                "\x1b[32m[MIC]\x1b[0m"
            },
            if self.tts_enabled {
                "\x1b[32m[TTS]\x1b[0m"
            } else {
                "\x1b[31m[TTS OFF]\x1b[0m"
            },
            if self.wake_enabled {
                "\x1b[32m[WAKE]\x1b[0m"
            } else {
                "\x1b[33m[NO WAKE]\x1b[0m"
            },
        );

        let style_name = match self.orb.style {
            OrbStyle::Rings => "Rings",
            OrbStyle::Blob => "Blob",
        };

        out.push_str(&format!(
            " \x1b[1m{}\x1b[0m | {} | {} | Style: {} | Ctx: {} | Resp: {}",
            self.status,
            mode_str,
            toggles,
            style_name,
            self.context_words,
            self.last_response_words
        ));

        // Input line
        out.push_str("\r\n");

        // Auto-submit progress bar
        if let Some(progress) = self.auto_submit_progress {
            const BLOCKS: &[char] = &[' ', '|', '|', '|', '|'];
            let total = 4;
            let filled = (progress * total as f32) as usize;
            let bar: String = (0..total)
                .map(|i| if i < filled { BLOCKS[4] } else { BLOCKS[0] })
                .collect();
            out.push_str(&format!("\x1b[33m[{}]\x1b[0m ", bar));
        }

        // Preview text
        if !self.preview.is_empty() {
            out.push_str(&format!("\x1b[90m{}\x1b[0m ", self.preview));
        }

        // Input prompt
        out.push_str(&format!("\x1b[32m>\x1b[0m {}", self.input));

        print!("{}", out);
        stdout().flush()?;

        Ok(())
    }

    fn poll_input(&mut self) -> io::Result<Option<String>> {
        let mut pending_submit = None;

        while event::poll(std::time::Duration::from_millis(0))? {
            if let Event::Key(key) = event::read()? {
                self.keypress_activity = true;

                if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    return Ok(Some("\x03".to_string()));
                }
                if key.code == KeyCode::Char('m') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    return Ok(Some("/mute".to_string()));
                }

                // Tab to switch visual style
                if key.code == KeyCode::Tab {
                    let new_style = match self.orb.style {
                        OrbStyle::Rings => OrbStyle::Blob,
                        OrbStyle::Blob => OrbStyle::Rings,
                    };
                    self.orb.set_style(new_style);
                    continue;
                }

                match key.code {
                    KeyCode::Enter => {
                        if event::poll(std::time::Duration::from_millis(0))? {
                            let byte_pos = self.char_to_byte_index(self.cursor_pos);
                            self.input.insert(byte_pos, '\n');
                            self.cursor_pos += 1;
                            self.input_activity = true;
                            pending_submit = None;
                        } else {
                            let text = self.input.trim().to_string();
                            self.input.clear();
                            self.cursor_pos = 0;
                            pending_submit = if !text.is_empty() { Some(text) } else { None };
                        }
                    }
                    KeyCode::Char(c) => {
                        if key.modifiers.contains(KeyModifiers::CONTROL) {
                            match c {
                                'a' => self.cursor_pos = 0,
                                'e' => self.cursor_pos = self.char_count(),
                                'k' => {
                                    if self.cursor_pos < self.char_count() {
                                        let byte_pos = self.char_to_byte_index(self.cursor_pos);
                                        self.input.truncate(byte_pos);
                                        self.input_activity = true;
                                    }
                                }
                                'u' => {
                                    if self.cursor_pos > 0 {
                                        let byte_pos = self.char_to_byte_index(self.cursor_pos);
                                        self.input = self.input[byte_pos..].to_string();
                                        self.cursor_pos = 0;
                                        self.input_activity = true;
                                    }
                                }
                                'w' => {
                                    if self.cursor_pos > 0 {
                                        let chars: Vec<char> = self.input.chars().collect();
                                        let mut end = self.cursor_pos;

                                        while end > 0 && chars[end - 1].is_whitespace() {
                                            end -= 1;
                                        }
                                        while end > 0 && !chars[end - 1].is_whitespace() {
                                            end -= 1;
                                        }

                                        let start_byte = self.char_to_byte_index(end);
                                        let end_byte = self.char_to_byte_index(self.cursor_pos);
                                        self.input.replace_range(start_byte..end_byte, "");
                                        self.cursor_pos = end;
                                        self.input_activity = true;
                                    }
                                }
                                _ => {}
                            }
                        } else {
                            let byte_pos = self.char_to_byte_index(self.cursor_pos);
                            self.input.insert(byte_pos, c);
                            self.cursor_pos += 1;
                            self.input_activity = true;
                        }
                    }
                    KeyCode::Backspace if self.cursor_pos > 0 => {
                        self.cursor_pos -= 1;
                        let byte_pos = self.char_to_byte_index(self.cursor_pos);
                        self.input.remove(byte_pos);
                        self.input_activity = true;
                    }
                    KeyCode::Delete if self.cursor_pos < self.char_count() => {
                        let byte_pos = self.char_to_byte_index(self.cursor_pos);
                        self.input.remove(byte_pos);
                        self.input_activity = true;
                    }
                    KeyCode::Left => self.cursor_pos = self.cursor_pos.saturating_sub(1),
                    KeyCode::Right if self.cursor_pos < self.char_count() => self.cursor_pos += 1,
                    KeyCode::Home => self.cursor_pos = 0,
                    KeyCode::End => self.cursor_pos = self.char_count(),
                    _ => {}
                }
            }
        }

        Ok(pending_submit)
    }

    fn restore(&self) -> io::Result<()> {
        execute!(stdout(), cursor::Show, terminal::LeaveAlternateScreen)?;
        terminal::disable_raw_mode()?;
        Ok(())
    }

    fn show_message(&mut self, text: &str) {
        // In graphical mode, we could show messages in a floating panel
        // For now, update status with last message
        if let Some(line) = text.lines().last() {
            self.status = line.to_string();
        }
    }

    fn set_auto_submit_progress(&mut self, progress: Option<f32>) {
        self.auto_submit_progress = progress;
    }

    fn set_mic_muted(&mut self, muted: bool) {
        self.mic_muted = muted;
    }

    fn set_tts_enabled(&mut self, enabled: bool) {
        self.tts_enabled = enabled;
    }

    fn set_wake_enabled(&mut self, enabled: bool) {
        self.wake_enabled = enabled;
    }

    fn set_mode(&mut self, mode: AppMode) {
        self.mode = mode;
    }

    fn set_ready(&mut self) {
        self.ready = true;
        self.status = "Ready".to_string();
    }

    fn set_last_response_words(&mut self, words: usize) {
        self.last_response_words = words;
    }

    fn set_audio_level(&mut self, level: f32) {
        self.audio_level = level;
    }

    fn set_tts_level(&mut self, level: f32) {
        self.tts_level = level;
    }

    fn has_input_activity(&mut self) -> bool {
        let activity = self.input_activity;
        self.input_activity = false;
        activity
    }

    fn has_keypress_activity(&mut self) -> bool {
        let activity = self.keypress_activity;
        self.keypress_activity = false;
        activity
    }

    fn take_input(&mut self) -> Option<String> {
        if self.input.is_empty() {
            None
        } else {
            let text = std::mem::take(&mut self.input);
            self.cursor_pos = 0;
            Some(text)
        }
    }

    fn append_input(&mut self, text: &str) {
        if !self.input.is_empty() && !self.input.ends_with(' ') {
            self.input.push(' ');
        }
        self.input.push_str(text);
        self.cursor_pos = self.char_count();
    }

    fn ui_mode(&self) -> UiMode {
        UiMode::Graphical
    }

    fn set_visual_style(&mut self, style: OrbStyle) {
        self.orb.set_style(style);
    }
}

impl Drop for GraphicalUi {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}
