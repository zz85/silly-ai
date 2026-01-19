//! Graphical orb UI for the voice assistant
//!
//! Provides a visual representation of the assistant's state using animated
//! ASCII art orbs. Supports multiple visual styles: Rings, Blob, and Ring.

use crate::render::{OrbStyle, UiEvent, UiMode, UiRenderer};
use crate::state::AppMode;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::style::Color;
use crossterm::terminal::{self, ClearType};
use crossterm::{cursor, execute};
use std::io::{self, Write, stdout};
use std::time::{Duration, Instant};
use std::thread;

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
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ShadePattern {
    BrailleAt,      // Braille with @
    Classic,        // Classic ASCII
    Circles,        // Unicode circles
    BrailleSolid,   // Braille with solid block
    Lines,          // Pipes and lines
    Particles,      // Optimized for particle rendering
}

impl ShadePattern {
    fn chars(&self) -> &'static [char] {
        match self {
            ShadePattern::BrailleAt => &[' ', '⠁', '⠃', '⠇', '⠏', '⠟', '⠿', '⣿', '@'],
            ShadePattern::Classic => &[' ', '.', ':', '-', '=', '+', '*', '#', '@'],
            ShadePattern::Circles => &[' ', '·', ':', '∘', '○', '●', '◉', '#', '@'],
            ShadePattern::BrailleSolid => &[' ', '⠁', '⠃', '⠇', '⠏', '⠟', '⠿', '⣿', '█'],
            ShadePattern::Lines => &[' ', '|', '¦', '‖', '║', '█', '█', '█', '█'],
            ShadePattern::Particles => &[' ', '·', '∘', '°', '○', '●', '◉', '⬢', '⬣'],
        }
    }

    fn name(&self) -> &'static str {
        match self {
            ShadePattern::BrailleAt => "Braille@",
            ShadePattern::Classic => "Classic",
            ShadePattern::Circles => "Circles",
            ShadePattern::BrailleSolid => "Braille█",
            ShadePattern::Lines => "Lines",
            ShadePattern::Particles => "Particles",
        }
    }

    fn next(&self) -> ShadePattern {
        match self {
            ShadePattern::BrailleAt => ShadePattern::Classic,
            ShadePattern::Classic => ShadePattern::Circles,
            ShadePattern::Circles => ShadePattern::BrailleSolid,
            ShadePattern::BrailleSolid => ShadePattern::Lines,
            ShadePattern::Lines => ShadePattern::Particles,
            ShadePattern::Particles => ShadePattern::BrailleAt,
        }
    }
}

impl OrbState {
    /// Animation frequency multiplier for each state.
    /// Higher values = faster animation.
    ///
    /// BPM reference (assuming base animation cycle is ~1 second):
    /// Some initial estimates
    /// - Idle:      0.4 Hz = ~24 BPM (calm, slow breathing)
    /// - Listening: 0.7 Hz = ~42 BPM (attentive, moderate pace)
    /// - Thinking:  1.4 Hz = ~84 BPM (active processing)
    /// - Speaking:  1.0 Hz = ~60 BPM (natural speech rhythm)
    /// - Error:     2.0 Hz = ~120 BPM (urgent, attention-grabbing)
    fn frequency(&self) -> f64 {
        match self {
            OrbState::Idle => 0.7,
            OrbState::Listening => 0.5,
            OrbState::Thinking => 1.0,
            OrbState::Speaking => 0.8,
            OrbState::Error => 1.5,
        }
    }

    fn palette(&self) -> Palette {
        match self {
            // Enhanced vibrant palette (commented out for now)
            /*
            OrbState::Idle => Palette {
                core: hsl(200.0, 0.85, 0.88),   // Soft sky blue core
                mid: hsl(210.0, 0.90, 0.72),    // Bright blue
                edge: hsl(220.0, 0.95, 0.58),   // Deep blue
                glow: hsl(195.0, 0.75, 0.42),   // Ocean blue glow
            },
            OrbState::Listening => Palette {
                core: hsl(150.0, 0.95, 0.85),   // Bright mint core
                mid: hsl(165.0, 0.90, 0.68),    // Vibrant teal
                edge: hsl(175.0, 0.85, 0.52),   // Deep cyan
                glow: hsl(160.0, 0.80, 0.38),   // Forest green glow
            },
            OrbState::Thinking => Palette {
                core: hsl(290.0, 0.90, 0.88),   // Bright lavender core
                mid: hsl(280.0, 0.95, 0.72),    // Vibrant purple
                edge: hsl(270.0, 0.95, 0.58),   // Deep purple
                glow: hsl(285.0, 0.85, 0.42),   // Royal purple glow
            },
            OrbState::Speaking => Palette {
                core: hsl(50.0, 1.0, 0.85),     // Bright golden core
                mid: hsl(40.0, 1.0, 0.72),      // Vibrant gold
                edge: hsl(30.0, 0.95, 0.58),    // Deep orange
                glow: hsl(35.0, 0.90, 0.42),    // Amber glow
            },
            OrbState::Error => Palette {
                core: hsl(5.0, 1.0, 0.85),      // Bright coral core
                mid: hsl(0.0, 0.95, 0.68),      // Vibrant red
                edge: hsl(355.0, 0.90, 0.52),   // Deep crimson
                glow: hsl(10.0, 0.85, 0.38),    // Dark red glow
            },
            */
            
            // Previous palette (restored)
            OrbState::Idle => Palette {
                core: hsl(220.0, 0.8, 0.85),    // Brighter blue core
                mid: hsl(230.0, 0.9, 0.70),     // Vibrant blue-purple
                edge: hsl(240.0, 0.95, 0.55),   // Deep blue-purple
                glow: hsl(210.0, 0.7, 0.40),    // Darker blue glow
            },
            OrbState::Listening => Palette {
                core: hsl(160.0, 0.95, 0.80),   // Bright cyan-green core
                mid: hsl(170.0, 0.90, 0.65),    // Vibrant teal
                edge: hsl(180.0, 0.85, 0.50),   // Deep cyan
                glow: hsl(165.0, 0.75, 0.35),   // Darker teal glow
            },
            OrbState::Thinking => Palette {
                core: hsl(280.0, 0.90, 0.85),   // Bright magenta core
                mid: hsl(270.0, 0.95, 0.70),    // Vibrant purple
                edge: hsl(260.0, 0.95, 0.55),   // Deep purple
                glow: hsl(275.0, 0.80, 0.40),   // Darker purple glow
            },
            OrbState::Speaking => Palette {
                core: hsl(45.0, 1.0, 0.80),     // Bright golden core
                mid: hsl(35.0, 1.0, 0.70),      // Vibrant orange-gold
                edge: hsl(25.0, 0.95, 0.55),    // Deep orange
                glow: hsl(30.0, 0.85, 0.40),    // Darker orange glow
            },
            OrbState::Error => Palette {
                core: hsl(0.0, 1.0, 0.80),      // Bright red core
                mid: hsl(10.0, 0.95, 0.65),     // Vibrant red-orange
                edge: hsl(5.0, 0.90, 0.50),     // Deep red
                glow: hsl(0.0, 0.80, 0.35),     // Darker red glow
            },
        }
    }
}


/// Represents a potentially composite state (e.g., listening + speaking)
#[derive(Clone, Copy)]
struct CompositeState {
    primary: OrbState,
    secondary: Option<OrbState>,
    blend: f64, // 0.0 = primary only, 1.0 = equal blend
}

impl CompositeState {
    fn single(state: OrbState) -> Self {
        Self {
            primary: state,
            secondary: None,
            blend: 0.0,
        }
    }

    fn dual(primary: OrbState, secondary: OrbState, blend: f64) -> Self {
        Self {
            primary,
            secondary: Some(secondary),
            blend: blend.clamp(0.0, 1.0),
        }
    }

    fn frequency(&self) -> f64 {
        match self.secondary {
            Some(sec) => {
                let f1 = self.primary.frequency();
                let f2 = sec.frequency();
                f1 * (1.0 - self.blend * 0.5) + f2 * self.blend * 0.5
            }
            None => self.primary.frequency(),
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
        // Improved color sampling with smoother transitions and better curve
        let t_clamped = t.clamp(0.0, 1.0);
        
        if t_clamped < 0.25 {
            // Core to mid transition - smooth curve
            let local_t = (t_clamped / 0.25).powf(0.8);
            self.core.lerp(self.mid, local_t)
        } else if t_clamped < 0.65 {
            // Mid to edge transition - linear for stability
            let local_t = (t_clamped - 0.25) / 0.4;
            self.mid.lerp(self.edge, local_t)
        } else {
            // Edge to glow transition - exponential for dramatic falloff
            let local_t = ((t_clamped - 0.65) / 0.35).powf(1.5);
            self.edge.lerp(self.glow, local_t)
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

    // Smoother interpolation (quintic instead of cubic)
    let u = xf * xf * xf * (xf * (xf * 6.0 - 15.0) + 10.0);
    let v = yf * yf * yf * (yf * (yf * 6.0 - 15.0) + 10.0);
    let w = zf * zf * zf * (zf * (zf * 6.0 - 15.0) + 10.0);

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
    
    // Normalize to [-1, 1] range for better organic movement
    (value / max_value) * 2.0 - 1.0
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
    composite: CompositeState,
    time: f64,
    transition: f64,
    audio_level: f64,
    audio_freqs: [f64; 8],
    smooth_audio: f64,
    smooth_freqs: [f64; 8],
    style: OrbStyle,
    secondary_audio: f64,
    smooth_secondary: f64,
    shade_pattern: ShadePattern,
    random_positions: Vec<(f64, f64, f64)>, // Pre-generated random sphere positions
}

impl Orb {
    fn new(style: OrbStyle) -> Self {
        // Generate random particles on sphere surface using hash-based pseudo-randomness
        let mut random_positions = Vec::new();
        let max_particles = 300; // Generate enough for the highest particle count (256) with some buffer
        
        for i in 0..max_particles {
            // Use hash function to generate pseudo-random values for each particle
            let seed = i as f64;
            
            // Generate random spherical coordinates using hash
            let u1 = hash(seed, 12.34, 56.78); // Random value [0,1]
            let u2 = hash(seed + 100.0, 78.90, 23.45); // Another random value [0,1]
            let u3 = hash(seed + 200.0, 34.56, 89.12); // Third random value for clustering
            
            // Add some clustering to make distribution more natural (not perfectly uniform)
            let cluster_strength = 0.15; // How much clustering to apply
            let cluster_u1 = u1 + (u3 - 0.5) * cluster_strength;
            let cluster_u2 = u2 + (hash(seed + 300.0, 45.67, 12.89) - 0.5) * cluster_strength;
            
            // Clamp to [0,1] range
            let final_u1 = cluster_u1.clamp(0.0, 1.0);
            let final_u2 = cluster_u2.clamp(0.0, 1.0);
            
            // Convert uniform random values to spherical coordinates
            // This ensures good distribution on sphere surface with natural clustering
            let theta = final_u1 * TAU; // Azimuthal angle [0, 2π]
            let phi = (2.0 * final_u2 - 1.0).acos(); // Polar angle [0, π] with proper distribution
            
            // Convert spherical to Cartesian coordinates
            let x = phi.sin() * theta.cos();
            let y = phi.sin() * theta.sin();
            let z = phi.cos();
            
            random_positions.push((x, y, z));
        }
        
        Self {
            state: OrbState::Idle,
            target_state: OrbState::Idle,
            composite: CompositeState::single(OrbState::Idle),
            time: 0.0,
            transition: 1.0,
            audio_level: 0.0,
            audio_freqs: [0.0; 8],
            smooth_audio: 0.0,
            smooth_freqs: [0.0; 8],
            style,
            secondary_audio: 0.0,
            smooth_secondary: 0.0,
            shade_pattern: ShadePattern::Particles,
            random_positions,
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

    fn set_shade_pattern(&mut self, pattern: ShadePattern) {
        self.shade_pattern = pattern;
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
        for i in 0..6 {
            // instead of 0..8 for inner rings
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

     // -------------------------------------------------------------------------
    // Ring style - thin horizontal elliptical rings with wave displacement
    //
    // The rings appear as if viewing a set of concentric circles from an angle,
    // creating wide but very short (thin) ellipses. Think of Saturn's rings
    // viewed from slightly above the plane.
    // -------------------------------------------------------------------------

    fn sample_rings2(&self, x: f64, y: f64, max_r: f64) -> (f64, f64, f64) {
        let freq = self.current_frequency();

        // =================================================================
        // SHAPE TRANSFORM - Improved ellipse calculation
        // =================================================================
        let x_squash = 0.35; // Optimized for better visual balance
        let y_squash = 1.2;  // Slight vertical compression for better proportions
        let x_scaled = x * x_squash;
        let y_scaled = y * y_squash;

        // =================================================================
        // ENHANCED WAVE DISPLACEMENT
        // Multi-layered wave system for more organic movement
        // =================================================================
        let wave_freq = 1.8; // Slightly lower for smoother motion
        let wave_speed = 0.35;
        let base_wave_amp = max_r * 0.02;
        
        // Primary wave
        let x_norm = x / max_r;
        let primary_wave = (x_norm * wave_freq + self.time * freq * TAU * wave_speed).sin();
        
        // Secondary harmonic for complexity
        let secondary_wave = (x_norm * wave_freq * 2.3 + self.time * freq * TAU * wave_speed * 1.7).sin() * 0.4;
        
        // Audio-reactive wave amplitude
        let audio_wave_boost = 1.0 + self.smooth_audio * 0.5;
        let combined_wave = (primary_wave + secondary_wave) * base_wave_amp * audio_wave_boost;
        
        let y_displaced = y_scaled + combined_wave;

        // =================================================================
        // COORDINATE SYSTEM - Improved distance calculation
        // =================================================================
        let dist = (x_scaled * x_scaled + y_displaced * y_displaced).sqrt();
        let angle = y_displaced.atan2(x_scaled);
        let r = dist / max_r;

        // Optimized early exit with smoother falloff
        if r > 0.8 {
            return (0.0, 0.0, 0.0);
        }

        let mut intensity = 0.0;
        let mut glow = 0.0;
        let mut secondary_intensity = 0.0;

        // =================================================================
        // ENHANCED CENTRAL CORE
        // Improved core with better falloff and audio reactivity
        // =================================================================
        let core_dist = ((x * x_squash).powi(2) + (y * y_squash * 0.9).powi(2)).sqrt() / max_r;
        
        // Multi-frequency breathing with harmonics
        let core_pulse = 1.0 + 0.12 * (self.time * freq * TAU).sin() 
                            + 0.06 * (self.time * freq * TAU * 2.1).sin();
        
        // Dynamic core size based on state and audio
        let base_core_size = match self.target_state {
            OrbState::Idle => 0.035,
            OrbState::Listening => 0.045,
            OrbState::Thinking => 0.055,
            OrbState::Speaking => 0.050,
            OrbState::Error => 0.040,
        };
        
        let core_size = (base_core_size + self.smooth_audio * 0.025) * core_pulse;
        
        // Improved Gaussian with smoother falloff
        let core_falloff = -(core_dist * core_dist) / (2.0 * core_size * core_size);
        let core = core_falloff.exp();
        
        intensity += core * 0.95;
        glow += core * 1.3;

        // Enhanced secondary core for dual-color mode
        if self.composite.secondary.is_some() {
            let sec_pulse = 1.0 + 0.10 * (self.time * freq * TAU + 2.1).sin();
            let sec_size = (base_core_size * 0.8 + self.smooth_secondary * 0.02) * sec_pulse;
            let sec_falloff = -(core_dist * core_dist) / (2.0 * sec_size * sec_size);
            let sec_core = sec_falloff.exp();
            secondary_intensity += sec_core * self.composite.blend * 0.85;
        }

        // =================================================================
        // IMPROVED CONCENTRIC RINGS
        // Better ring distribution and audio reactivity
        // =================================================================
        let ring_count = match self.target_state {
            OrbState::Idle => 3,
            OrbState::Listening => 4,
            OrbState::Thinking => 6, // More rings for complexity
            OrbState::Speaking => 5,
            OrbState::Error => 4,
        };

        // Improved ring spacing with non-linear distribution
        let inner_r = 0.18;
        let outer_r = 0.42;

        for i in 0..ring_count {
            let ring_phase = i as f64 / (ring_count - 1).max(1) as f64;
            
            // Non-linear ring distribution for better visual balance
            let ring_curve = ring_phase.powf(1.2);
            let base_r = inner_r + ring_curve * (outer_r - inner_r);

            // Enhanced breathing with per-ring phase offset
            let breath_phase = self.time * freq * TAU + ring_phase * TAU * 1.8 + i as f64 * 0.5;
            let breath = breath_phase.sin() * (0.008 + ring_phase * 0.006);

            // Improved audio reactivity with frequency separation
            let band = (i * 6 / ring_count).min(5); // Use fewer bands for smoother response
            let audio_r = self.smooth_freqs[band] * (0.02 + ring_phase * 0.01);

            // Enhanced organic wobble
            let wobble = self.ring_wobble(angle, i, self.time * freq) * (1.0 + self.smooth_audio * 0.3);

            let ring_r = base_r + breath + audio_r + wobble;

            // =================================================================
            // ENHANCED RING INTENSITY
            // Improved falloff and visual quality
            // =================================================================
            let adaptive_width = 0.008 + self.smooth_audio * 0.004 + ring_phase * 0.002;
            let d = (r - ring_r).abs();
            let ring_falloff = -(d * d) / (2.0 * adaptive_width * adaptive_width);
            let ring_intensity = ring_falloff.exp();

            // Improved fade with smoother transition
            let fade = (1.0 - ring_phase * 0.3).max(0.4);

            // Enhanced edge brightness with better curve
            let edge_y = (y / max_r).abs();
            let edge_bright = 0.75 + edge_y.powf(0.7) * 0.35;

            // State-dependent intensity scaling
            let state_intensity = match self.target_state {
                OrbState::Thinking => 0.65,
                OrbState::Speaking => 0.60,
                OrbState::Listening => 0.70,
                _ => 0.55,
            };

            intensity += ring_intensity * fade * edge_bright * state_intensity;
            glow += ring_intensity * fade * 0.25;

            // Enhanced secondary color system
            if self.composite.secondary.is_some() && i % 2 == 1 {
                let sec_audio = self.smooth_secondary * 0.025;
                let sec_wobble = self.ring_wobble(angle, i + 13, self.time * freq * 1.15);
                let sec_r = ring_r * 0.96 + sec_audio + sec_wobble;
                let sec_d = (r - sec_r).abs();
                let sec_falloff = -(sec_d * sec_d) / (2.0 * adaptive_width * adaptive_width);
                let sec_ring = sec_falloff.exp();
                secondary_intensity += sec_ring * fade * 0.5 * self.composite.blend;
            }
        }

        // =================================================================
        // ENHANCED AMBIENT GLOW
        // Improved atmospheric effect with distance-based falloff
        // =================================================================
        let glow_distance = (r - outer_r * 0.9).max(0.0);
        let ambient_falloff = -(glow_distance * glow_distance * 20.0);
        let ambient = ambient_falloff.exp() * 0.08;
        glow += ambient;

        // Add subtle noise-based atmospheric scattering
        let noise_x = x / max_r * 3.0;
        let noise_y = y / max_r * 3.0;
        let noise_t = self.time * 0.2;
        let atmospheric_noise = smooth_noise(noise_x, noise_y, noise_t) * 0.02;
        glow += atmospheric_noise * (1.0 - r).max(0.0);

        (
            intensity.min(1.0),
            glow.min(1.0),
            secondary_intensity.min(1.0),
        )
    }

    fn ring_wobble(&self, angle: f64, ring_idx: usize, t: f64) -> f64 {
        let mut w = 0.0;
        
        // Multi-harmonic wobble for more organic movement
        for h in 1..=5 {
            let hf = h as f64;
            let speed = 0.25 + (ring_idx as f64 * 0.05);
            let phase = t * hf * speed + ring_idx as f64 * 0.8;
            
            // Different harmonics have different amplitudes for natural variation
            let amplitude = match h {
                1 => 0.012, // Fundamental
                2 => 0.008, // Second harmonic
                3 => 0.005, // Third harmonic
                4 => 0.003, // Fourth harmonic
                _ => 0.002, // Higher harmonics
            };
            
            w += (angle * hf * 1.8 + phase * TAU).sin() * amplitude / hf.sqrt();
        }
        
        // Enhanced audio reactivity with smoother frequency mapping
        let angle_norm = (angle / TAU + 0.5).fract();
        let band = (angle_norm * 6.0) as usize; // Use 6 bands for smoother distribution
        let audio_wobble = self.smooth_freqs[band] * (0.018 + ring_idx as f64 * 0.002);
        
        // Add subtle noise for organic variation
        let noise_factor = smooth_noise(
            angle * 2.0, 
            ring_idx as f64 * 0.5, 
            t * 0.3
        ) * 0.004;
        
        w + audio_wobble + noise_factor
    }

    // Blob style renderer - volumetric noise blob
    fn sample_blob(&self, x: f64, y: f64, max_r: f64) -> (f64, f64) {
        let dist = (x * x + y * y).sqrt();
        let angle = y.atan2(x);
        let r = dist / max_r;

        if r > 1.4 {
            return (0.0, 0.0);
        }

        let freq = self.current_frequency();
        let t = self.time * freq;
        let angle = angle + (t * 0.1).sin() * 2.;

        let noise_scale = match self.target_state {
            OrbState::Idle => 1.5,
            OrbState::Listening => 2.0,
            OrbState::Thinking => 3.0,
            OrbState::Speaking => 2.2,
            OrbState::Error => 3.5,
        };

        let octaves = match self.target_state {
            OrbState::Idle => 3,
            OrbState::Thinking => 5,
            OrbState::Error => 4,
            _ => 4,
        };

        let nx = angle.cos() * noise_scale;
        let ny = angle.sin() * noise_scale;
        let nz = t * 0.8;

        let noise = fbm(nx, ny, nz, octaves, 0.5);

        // Base radius - affected by smooth_secondary when Speaking
        let speaking_expansion = if self.target_state == OrbState::Speaking {
            self.smooth_secondary
        } else {
            0.0
        };
        let base_radius = 0.55 + self.smooth_audio + speaking_expansion;
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

    // Ring style renderer - rotating ring with subtle wobble
    // Single glowing ring that rotates and has very subtle organic wobble
    fn sample_rings1(&self, x: f64, y: f64, max_r: f64) -> (f64, f64) {
        let y = y * 1.3;
        let dist = (x * x + y * y).sqrt();
        let angle = y.atan2(x);
        let r = dist / max_r;

        if r > 1.2 {
            return (0.0, 0.0);
        }

        let freq = self.current_frequency();
        let t = self.time * freq;
        let vol = self.smooth_secondary + self.smooth_audio ;
        let angle = (vol * (1. * t + vol * 5.).cos()).sin() * 7. + angle;

        // Base ring radius - stable with slight audio reactivity
        let speaking_expansion = if self.target_state == OrbState::Speaking {
            self.smooth_secondary * 0.3
        } else {
            0.0
        };
        let base_ring_r = 0.5 + self.smooth_audio * 0.2 + speaking_expansion;

        // Rotation-based animation instead of pulsing
        // Create rotating pattern by modulating ring radius based on angle and time
        let rotation_speed = match self.target_state {
            OrbState::Idle => 0.3,
            OrbState::Listening => 0.5,
            OrbState::Thinking => 0.8,
            OrbState::Speaking => 0.6,
            OrbState::Error => 1.2,
        } * 0.1;

        // Multi-harmonic rotation for more interesting patterns
        let rotation_phase = t * rotation_speed * 0.5;
        let rotation_modulation = 
            (angle * 3.0 + rotation_phase * TAU).sin() * 0.04 +
            (angle * 5.0 - rotation_phase * TAU * 0.7).sin() * 0.025 +
            (angle * 7.0 + rotation_phase * TAU * 1.3).sin() * 0.015;

        // Very subtle wobble - reduced to maintain ring shape
        let subtle_wobble = self.ring_wobble(angle, 0, t * 100.);

        // Final ring radius with rotation and very subtle wobble
        let ring_r = base_ring_r + rotation_modulation * 0.15 + subtle_wobble * 0.1;

        // Ring width varies with audio and state
        let base_width = match self.target_state {
            OrbState::Idle => 0.06,
            OrbState::Listening => 0.08,
            OrbState::Thinking => 0.05,
            OrbState::Speaking => 0.07,
            OrbState::Error => 0.04,
        } * 0.04;
        let ring_width = base_width + self.smooth_audio * 0.3 + self.smooth_secondary * 0.3;

        // Distance from ring with Gaussian falloff
        let ring_dist = (r - ring_r).abs();
        let ring_intensity = (-ring_dist * ring_dist / (2.0 * ring_width * ring_width)).exp();

        // Enhanced core with rotation-based brightness variation
        let core_size = 0.12 + self.smooth_audio * 0.08;
        let core_brightness_mod = 1.0 + (rotation_phase * 2.0).sin() * 0.15;
        let core = (-(r * r) / (2.0 * core_size * core_size)).exp() * 0.7 * core_brightness_mod;

        // Outer glow with rotation-based variation
        let outer_glow = if r > ring_r {
            let glow_strength = 1.0 + (angle * 2.0 + rotation_phase).sin() * 0.2 * 0.;
            (-(r - ring_r) * 2.5).exp() * 0.35 * glow_strength
        } else {
            0.0
        };

        // Brightness varies around the ring based on rotation
        let angular_brightness = 1.0 + (angle * 4.0 + rotation_phase * 1.5).sin() * 0.2;

        let intensity = (ring_intensity * 0.85 * angular_brightness + core).min(1.0);
        let glow = (ring_intensity * 0.25 + outer_glow).min(1.0);

        (intensity, glow)
    }

    // Simple wireframe sphere renderer - minimal particles at key positions
    fn sample_sphere(&self, x: f64, y: f64, max_r: f64) -> (f64, f64, f64) {
        let freq = self.current_frequency();
        let time_scale = self.time * freq;
        
        // SPHERE-SPECIFIC SCALING: Make sphere larger but keep particles visible
        let sphere_scale = 3.0;  // Reduced from 20x to 3x for better visibility
        let x_scaled = x / sphere_scale;  // Scale down coordinates = camera further away
        let y_scaled = y / sphere_scale;
        let max_r_scaled = max_r / sphere_scale;
        
        // Normalize coordinates for true sphere (using scaled values)
        let x_norm = x_scaled / max_r_scaled;
        let y_norm = y_scaled / max_r_scaled;
        let screen_dist = (x_norm * x_norm + y_norm * y_norm).sqrt();
        
        // Early exit for points outside sphere (adjusted for larger sphere)
        if screen_dist > 0.9 {
            return (0.0, 0.0, 0.0);
        }
        
        let mut intensity = 0.0;
        let mut glow = 0.0;
        let mut secondary_intensity = 0.0;
        
        // Use pre-generated random positions for natural distribution
        let key_positions = &self.random_positions;
        
        let particle_count = match self.target_state {
            OrbState::Idle => 80,        // Reduced slightly for stability
            OrbState::Listening => 120,  // Reduced slightly
            OrbState::Thinking => 180,   // Reduced slightly
            OrbState::Speaking => 140,   // Reduced slightly
            OrbState::Error => 200,      // Reduced from 256 to 200
        };
        
        // Render only the first N positions based on state
        for i in 0..particle_count.min(key_positions.len()) {
            let (base_x, base_y, base_z) = key_positions[i];
            
            // ORGANIC ANIMATION - Based on vertex displacement techniques from Three.js examples
            
            // Use particle's world position as noise coordinates for consistent organic movement
            let world_pos_x = base_x;
            let world_pos_y = base_y;
            let world_pos_z = base_z;
            
            // Time-based noise evolution for flowing animation
            let time_offset = time_scale * 0.5;
            
            // Multi-octave noise for complex organic movement
            let noise_scale_1 = 1.5;  // Primary frequency
            let noise_scale_2 = 3.0;  // Secondary frequency
            let noise_scale_3 = 6.0;  // Detail frequency
            
            // Sample noise at different frequencies (like shader vertex displacement)
            let noise_1 = fbm(
                world_pos_x * noise_scale_1 + time_offset,
                world_pos_y * noise_scale_1 + time_offset,
                world_pos_z * noise_scale_1 + time_offset,
                4, 0.5
            );
            
            let noise_2 = fbm(
                world_pos_x * noise_scale_2 + time_offset * 1.3,
                world_pos_y * noise_scale_2 + time_offset * 1.3,
                world_pos_z * noise_scale_2 + time_offset * 1.3,
                3, 0.6
            );
            
            let noise_3 = fbm(
                world_pos_x * noise_scale_3 + time_offset * 0.7,
                world_pos_y * noise_scale_3 + time_offset * 0.7,
                world_pos_z * noise_scale_3 + time_offset * 0.7,
                2, 0.4
            );
            
            // Combine noise layers for complex organic movement with turbulence
            let combined_noise = noise_1 * 0.6 + noise_2 * 0.3 + noise_3 * 0.1;
            
            // Add turbulence for more organic, flowing movement
            let turbulence_x = fbm(
                world_pos_y * 2.0 + time_offset * 0.8,
                world_pos_z * 2.0 + time_offset * 0.8,
                world_pos_x * 2.0 + time_offset * 0.8,
                2, 0.5
            ).clamp(-1.0, 1.0) * 0.15;
            
            let turbulence_y = fbm(
                world_pos_z * 2.0 + time_offset * 1.1,
                world_pos_x * 2.0 + time_offset * 1.1,
                world_pos_y * 2.0 + time_offset * 1.1,
                2, 0.5
            ).clamp(-1.0, 1.0) * 0.15;
            
            let turbulence_z = fbm(
                world_pos_x * 2.0 + time_offset * 0.9,
                world_pos_y * 2.0 + time_offset * 0.9,
                world_pos_z * 2.0 + time_offset * 0.9,
                2, 0.5
            ).clamp(-1.0, 1.0) * 0.15;
            
            // State-dependent noise amplitude
            let noise_amplitude = match self.target_state {
                OrbState::Idle => 0.08,
                OrbState::Listening => 0.15,
                OrbState::Thinking => 0.25,
                OrbState::Speaking => 0.18,
                OrbState::Error => 0.35,
            };
            
            // Apply noise as radial displacement (like vertex shader displacement)
            let displacement = (combined_noise * noise_amplitude).clamp(-0.5, 0.5); // Clamp displacement
            
            // Audio-reactive enhancement
            let audio_band = i % 6;
            let audio_displacement = (self.smooth_freqs[audio_band] * 0.12).clamp(-0.2, 0.2); // Clamp audio displacement
            
            // Breathing effect with organic variation
            let breathing_base = (time_scale * 1.2).sin() * 0.04;
            let breathing_variation = (time_scale * 2.1 + i as f64 * 0.4).sin() * 0.02;
            let breathing = breathing_base + breathing_variation;
            
            // Final organic radius calculation
            let base_radius = 1.0;
            let organic_radius = base_radius + displacement + audio_displacement + breathing;
            let final_radius = organic_radius.max(0.2); // Prevent collapse
            
            // Apply organic displacement to particle position with turbulence
            let organic_x = world_pos_x + turbulence_x * noise_amplitude;
            let organic_y = world_pos_y + turbulence_y * noise_amplitude;
            let organic_z = world_pos_z + turbulence_z * noise_amplitude;
            
            // Normalize to maintain sphere shape while allowing organic deformation
            let organic_length = (organic_x * organic_x + organic_y * organic_y + organic_z * organic_z).sqrt();
            
            // Prevent division by zero
            let safe_length = if organic_length < 0.001 { 1.0 } else { organic_length };
            
            let normalized_x = organic_x / safe_length;
            let normalized_y = organic_y / safe_length;
            let normalized_z = organic_z / safe_length;
            
            let particle_x = normalized_x * final_radius;
            let particle_y = normalized_y * final_radius;
            let particle_z = normalized_z * final_radius;
            
            // Project to 2D (no aspect distortion)
            let proj_x = particle_x;
            let proj_y = particle_y;
            
            // Distance from current pixel to particle
            let particle_dist = ((x_norm - proj_x).powi(2) + (y_norm - proj_y).powi(2)).sqrt();
            
            // Particle size based on depth and state - 20x SMALLER particles for distant viewing
            let depth_factor = (particle_z + 1.0) * 0.5; // Normalize z to [0,1]
            let size_factor = 0.6 + depth_factor * 0.8; // Closer = larger
            let brightness_factor = 0.3 + depth_factor * 0.7;
            
            let base_size = match self.target_state {
                OrbState::Idle => 0.018,       // Slightly smaller for denser appearance
                OrbState::Listening => 0.016,   
                OrbState::Thinking => 0.013,     
                OrbState::Speaking => 0.020,    
                OrbState::Error => 0.022,        
            };
            
            let dynamic_size = base_size * size_factor * (1.0 + self.smooth_audio * 0.2);
            
            // Only render if we're close enough to the particle
            if particle_dist <= dynamic_size {
                // Gaussian falloff for smooth particle edges
                let falloff = -(particle_dist * particle_dist) / (2.0 * dynamic_size * dynamic_size * 0.5);
                let particle_intensity = falloff.exp() * brightness_factor;
                
                intensity += particle_intensity * 0.8;
                glow += particle_intensity * 0.3;
                
                // Secondary color for dual-state rendering
                if self.composite.secondary.is_some() && i % 2 == 0 {
                    secondary_intensity += particle_intensity * 0.6 * self.composite.blend;
                }
            }
        }
        
        // No central core - pure particle sphere approach
        // No outer glow - clean particle-only rendering
        
        (intensity.min(1.0), glow.min(1.0), secondary_intensity.min(1.0))
    }

    fn render(&self, width: usize, height: usize) -> Vec<Vec<(char, Color)>> {
        let mut buffer = vec![vec![(' ', Color::Reset); width]; height];
        let palette = self.current_palette();

        let aspect = 2.0; // Slightly adjusted for better proportions
        let max_r = (height as f64).min(width as f64 / aspect) * 0.48;
        let cx = width as f64 / 2.0;
        let cy = height as f64 / 2.0;

        let shades = self.shade_pattern.chars();

        for row in 0..height {
            for col in 0..width {
                let x = (col as f64 - cx) / aspect;
                let y = row as f64 - cy;

                let (intensity, glow, secondary) = match self.style {
                    OrbStyle::Blob => {
                        let (a, b) = self.sample_blob(x, y, max_r);
                        (a, b, 0.0)
                    },
                    OrbStyle::Ring => {
                        let (a, b) = self.sample_rings1(x, y, max_r);
                        (a, b, 0.0)
                    },
                    OrbStyle::Orbs => self.sample_rings2(x, y, max_r),
                    OrbStyle::Sphere => self.sample_sphere(x, y, max_r),
                };

                // Skip pixels with minimal contribution
                if intensity < 0.008 && glow < 0.015 && secondary < 0.008 {
                    continue;
                }

                // For sphere style, use particle-based coloring instead of distance-based
                let color_t = if matches!(self.style, OrbStyle::Sphere) {
                    // Use intensity for color variation in sphere mode
                    (intensity * 0.8 + glow * 0.2).min(1.0)
                } else {
                    // Use distance-based coloring for other styles
                    let dist = (x * x + y * y).sqrt() / max_r;
                    (dist * 1.05).min(1.0)
                };

                let base_color = palette.sample(color_t);
                
                // Enhanced color mixing for secondary colors
                let mut final_color = if secondary > 0.01 && self.composite.secondary.is_some() {
                    let secondary_palette = self.composite.secondary.unwrap().palette();
                    let sec_color = secondary_palette.sample(color_t);
                    base_color.lerp(sec_color, secondary * 0.7)
                } else {
                    base_color
                };

                // Enhanced brightness calculation with more vibrant colors
                let brightness = intensity * 1.0 + glow * 0.6 + secondary * 0.8;
                final_color = final_color.scale(0.4 + brightness * 0.6);

                // Enhanced highlight system with more vibrant highlights
                let combined = intensity + secondary * 0.8;
                if combined > 0.5 {
                    let highlight_strength = ((combined - 0.5) / 0.5).min(1.0);
                    let highlight = Rgb(
                        highlight_strength * 0.4,
                        highlight_strength * 0.4,
                        highlight_strength * 0.4
                    );
                    final_color = final_color.add(highlight);
                }

                // Improved character selection with better intensity mapping
                let char_intensity = (intensity + glow * 0.3 + secondary * 0.5).min(1.0);
                let char_curve = char_intensity.powf(0.7); // Gamma correction for better visual distribution
                let idx = ((char_curve * (shades.len() - 1) as f64).round() as usize)
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
            orb: Orb::new(OrbStyle::Sphere),
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
            OrbStyle::Orbs => "Orbs",
            OrbStyle::Blob => "Blob",
            OrbStyle::Ring => "Ring",
            OrbStyle::Sphere => "Sphere",
        };

        out.push_str(&format!(
            " \x1b[1m{}\x1b[0m | {} | {} | Style: {} | Shades: {} | Ctx: {} | Resp: {}",
            self.status,
            mode_str,
            toggles,
            style_name,
            self.orb.shade_pattern.name(),
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

                // Tab to cycle through visual styles
                if key.code == KeyCode::Tab {
                    let new_style = match self.orb.style {
                        OrbStyle::Orbs => OrbStyle::Blob,
                        OrbStyle::Blob => OrbStyle::Ring,
                        OrbStyle::Ring => OrbStyle::Sphere,
                        OrbStyle::Sphere => OrbStyle::Orbs,
                    };
                    self.orb.set_style(new_style);
                    continue;
                }

                // Backtick to cycle through shade patterns
                if key.code == KeyCode::Char('`') {
                    let new_pattern = self.orb.shade_pattern.next();
                    self.orb.set_shade_pattern(new_pattern);
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    /// Interactive test to showcase all orb states and styles
    /// Run with: cargo test --bin silly-cli graphical_ui_demo -- --nocapture --ignored
    #[test]
    #[ignore]
    fn graphical_ui_demo() {
        println!("Starting Graphical UI Demo...");
        println!("This will cycle through all orb states and styles.");
        println!("Press Ctrl+C to exit at any time.");

        let mut ui = GraphicalUi::new().expect("Failed to initialize UI");
        
        let states = [
            (OrbState::Idle, "Idle - Calm breathing"),
            (OrbState::Listening, "Listening - Attentive"),
            (OrbState::Thinking, "Thinking - Processing"),
            (OrbState::Speaking, "Speaking - Responding"),
            (OrbState::Error, "Error - Alert"),
        ];

        let styles = [
            (OrbStyle::Orbs, "Orbs - Concentric glowing orbs"),
            (OrbStyle::Blob, "Blob - Organic noise blob"),
            (OrbStyle::Ring, "Ring - Single rotating ring"),
        ];

        let mut frame_count = 0;
        let mut state_index = 0;
        let mut style_index = 0;
        let frames_per_state = 180; // 3 seconds at 60fps
        let frames_per_style = frames_per_state * states.len();

        loop {
            let now = std::time::Instant::now();

            // Check for Ctrl+C
            if let Ok(Some(input)) = ui.poll_input() {
                if input == "\x03" {
                    break;
                }
            }

            // Cycle through styles every few seconds
            if frame_count % frames_per_style == 0 {
                let (style, style_name) = styles[style_index];
                ui.orb.set_style(style);
                println!("\n=== Style: {} ===", style_name);
                style_index = (style_index + 1) % styles.len();
            }

            // Cycle through states within each style
            if frame_count % frames_per_state == 0 {
                let (state, state_name) = states[state_index];
                ui.orb.set_state(state);
                ui.status = format!("{} (Frame: {})", state_name, frame_count);
                state_index = (state_index + 1) % states.len();
            }

            // Simulate audio levels for more interesting visuals
            let audio_phase = frame_count as f64 * 0.1;
            let audio_level = (0.3 + 0.4 * (audio_phase * 0.5).sin()).max(0.0) as f32;
            let tts_level = (0.2 + 0.3 * (audio_phase * 0.7 + 1.0).sin()).max(0.0) as f32;
            
            ui.set_audio_level(audio_level);
            ui.set_tts_level(tts_level);

            // Update orb animation
            let dt = 1.0 / 60.0; // 60 FPS
            ui.orb.set_audio(audio_level as f64);
            ui.orb.set_secondary_audio(tts_level as f64);
            ui.orb.update(dt);

            // Draw frame
            if let Err(e) = ui.draw() {
                eprintln!("Draw error: {}", e);
                break;
            }

            frame_count += 1;

            // Maintain 60 FPS
            let elapsed = now.elapsed();
            let target_frame_time = Duration::from_millis(16); // ~60 FPS
            if elapsed < target_frame_time {
                thread::sleep(target_frame_time - elapsed);
            }
        }

        println!("\nDemo finished!");
    }

    /// Test individual orb rendering without UI setup
    #[test]
    fn test_orb_rendering() {
        let mut orb = Orb::new(OrbStyle::Rings2);
        
        // Test all states
        for state in [OrbState::Idle, OrbState::Listening, OrbState::Thinking, OrbState::Speaking, OrbState::Error] {
            orb.set_state(state);
            orb.set_audio(0.5);
            orb.update(0.016); // One frame
            
            let buffer = orb.render(80, 24);
            
            // Verify buffer dimensions
            assert_eq!(buffer.len(), 24);
            assert_eq!(buffer[0].len(), 80);
            
            // Check that some pixels are rendered (not all spaces)
            let has_content = buffer.iter().any(|row| {
                row.iter().any(|(ch, _)| *ch != ' ')
            });
            assert!(has_content, "State {:?} should render some content", state);
        }
    }

    /// Test all orb styles
    #[test]
    fn test_orb_styles() {
        for style in [OrbStyle::Ring, OrbStyle::Orbs, OrbStyle::Blob] {
            let mut orb = Orb::new(style);
            orb.set_state(OrbState::Thinking);
            orb.set_audio(0.7);
            orb.update(0.016);
            
            let buffer = orb.render(60, 20);
            
            // Verify rendering works for each style
            let has_content = buffer.iter().any(|row| {
                row.iter().any(|(ch, _)| *ch != ' ')
            });
            assert!(has_content, "Style {:?} should render content", style);
        }
    }

    /// Benchmark rendering performance
    #[test]
    #[ignore]
    fn benchmark_rendering() {
        let mut orb = Orb::new(OrbStyle::Orbs);
        orb.set_state(OrbState::Thinking);
        orb.set_audio(0.5);
        
        let start = std::time::Instant::now();
        let iterations = 1000;
        
        for _ in 0..iterations {
            orb.update(0.016);
            let _buffer = orb.render(80, 24);
        }
        
        let elapsed = start.elapsed();
        let fps = iterations as f64 / elapsed.as_secs_f64();
        
        println!("Rendered {} frames in {:?}", iterations, elapsed);
        println!("Average FPS: {:.2}", fps);
        
        // Should be able to render at least 60 FPS
        assert!(fps > 60.0, "Rendering too slow: {:.2} FPS", fps);
    }
}

/// Standalone demo function that can be called from main
pub fn run_orb_demo() -> io::Result<()> {
    println!("=== Orb Visual Demo ===");
    println!("Cycling through all states and styles...");
    println!("Press Tab to cycle styles, ` (backtick) to cycle shade patterns, Ctrl+C to exit");
    
    let mut ui = GraphicalUi::new()?;
    
    let states = [
        (OrbState::Idle, "Idle"),
        (OrbState::Listening, "Listening"), 
        (OrbState::Thinking, "Thinking"),
        (OrbState::Speaking, "Speaking"),
        (OrbState::Error, "Error"),
    ];
    
    let mut frame_count = 0;
    let mut state_index = 0;
    let auto_cycle = true;
    
    loop {
        let now = std::time::Instant::now();
        
        // Handle input
        if let Ok(Some(input)) = ui.poll_input() {
            if input == "\x03" {
                break;
            }
            // Tab key cycles styles (handled in poll_input)
        }
        
        // Auto-cycle states every 3 seconds
        if auto_cycle && frame_count % 180 == 0 {
            let (state, state_name) = states[state_index];
            ui.orb.set_state(state);
            ui.status = format!("{} - Auto Demo", state_name);
            state_index = (state_index + 1) % states.len();
        }
        
        // Simulate varying audio levels
        let t = frame_count as f64 * 0.05;
        let audio = (0.2 + 0.5 * (t * 0.8).sin() + 0.2 * (t * 1.3).sin()).max(0.0).min(1.0);
        let tts = (0.1 + 0.4 * (t * 0.6 + 2.0).sin()).max(0.0).min(1.0);
        
        ui.set_audio_level(audio as f32);
        ui.set_tts_level(tts as f32);
        
        // Update and draw
        ui.orb.set_audio(audio);
        ui.orb.set_secondary_audio(tts);
        ui.orb.update(1.0 / 60.0);
        
        ui.draw()?;
        
        frame_count += 1;
        
        // 60 FPS timing
        let elapsed = now.elapsed();
        let target = Duration::from_millis(16);
        if elapsed < target {
            thread::sleep(target - elapsed);
        }
    }
    
    Ok(())
}
