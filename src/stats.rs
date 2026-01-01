//! Performance stats tracking for inference operations

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Default)]
pub struct InferenceStats {
    pub transcription: Vec<Sample>,
    pub tts: Vec<Sample>,
    pub llm: Vec<LlmSample>,
}

#[derive(Clone)]
pub struct Sample {
    pub duration: Duration,
    pub input_size: usize,  // chars for TTS, samples for transcription
    pub output_size: usize, // chars for transcription, samples for TTS
}

#[derive(Clone)]
pub struct LlmSample {
    pub ttft: Duration,  // time to first token
    pub total: Duration, // total generation time
    pub tokens: usize,   // output tokens (approx by words)
}

impl InferenceStats {
    pub fn summary(&self) -> String {
        let mut out = String::new();

        if !self.transcription.is_empty() {
            let (avg, min, max, total) = Self::calc_duration(&self.transcription);
            let avg_samples: f64 = self
                .transcription
                .iter()
                .map(|s| s.input_size as f64)
                .sum::<f64>()
                / self.transcription.len() as f64;
            let rtf = avg.as_secs_f64() / (avg_samples / 16000.0);
            out.push_str(&format!(
                "Transcription (n={}): avg={:.0}ms min={:.0}ms max={:.0}ms RTF={:.2}x\n",
                self.transcription.len(),
                avg.as_millis(),
                min.as_millis(),
                max.as_millis(),
                rtf
            ));
        }

        if !self.tts.is_empty() {
            let (avg, min, max, _) = Self::calc_duration(&self.tts);
            let avg_out: f64 =
                self.tts.iter().map(|s| s.output_size as f64).sum::<f64>() / self.tts.len() as f64;
            let rtf = avg.as_secs_f64() / (avg_out / 24000.0);
            out.push_str(&format!(
                "TTS (n={}): avg={:.0}ms min={:.0}ms max={:.0}ms RTF={:.2}x\n",
                self.tts.len(),
                avg.as_millis(),
                min.as_millis(),
                max.as_millis(),
                rtf
            ));
        }

        if !self.llm.is_empty() {
            let avg_ttft: Duration =
                self.llm.iter().map(|s| s.ttft).sum::<Duration>() / self.llm.len() as u32;
            let total_time: Duration = self.llm.iter().map(|s| s.total).sum();
            let total_tokens: usize = self.llm.iter().map(|s| s.tokens).sum();
            let tps = if total_time.as_secs_f64() > 0.0 {
                total_tokens as f64 / total_time.as_secs_f64()
            } else {
                0.0
            };
            out.push_str(&format!(
                "LLM (n={}): TTFT={:.0}ms avg, {:.1} tok/s ({} tokens)\n",
                self.llm.len(),
                avg_ttft.as_millis(),
                tps,
                total_tokens
            ));
        }

        if out.is_empty() {
            out.push_str("No stats recorded yet.\n");
        }
        out
    }

    fn calc_duration(samples: &[Sample]) -> (Duration, Duration, Duration, Duration) {
        let total: Duration = samples.iter().map(|s| s.duration).sum();
        let avg = total / samples.len() as u32;
        let min = samples.iter().map(|s| s.duration).min().unwrap_or_default();
        let max = samples.iter().map(|s| s.duration).max().unwrap_or_default();
        (avg, min, max, total)
    }
}

pub type SharedStats = Arc<Mutex<InferenceStats>>;

pub fn new_shared() -> SharedStats {
    Arc::new(Mutex::new(InferenceStats::default()))
}

/// Timer helper that records on drop
pub struct Timer<'a> {
    start: Instant,
    stats: &'a SharedStats,
    kind: StatKind,
    input_size: usize,
}

pub enum StatKind {
    Transcription,
    Tts,
}

impl<'a> Timer<'a> {
    pub fn new(stats: &'a SharedStats, kind: StatKind, input_size: usize) -> Self {
        Self {
            start: Instant::now(),
            stats,
            kind,
            input_size,
        }
    }

    pub fn finish(self, output_size: usize) {
        let sample = Sample {
            duration: self.start.elapsed(),
            input_size: self.input_size,
            output_size,
        };
        let mut stats = self.stats.lock().unwrap();
        match self.kind {
            StatKind::Transcription => stats.transcription.push(sample),
            StatKind::Tts => stats.tts.push(sample),
        }
    }
}

/// LLM timing tracker
pub struct LlmTimer {
    start: Instant,
    first_token: Option<Instant>,
    stats: SharedStats,
}

impl LlmTimer {
    pub fn new(stats: SharedStats) -> Self {
        Self {
            start: Instant::now(),
            first_token: None,
            stats,
        }
    }

    pub fn mark_first_token(&mut self) {
        if self.first_token.is_none() {
            self.first_token = Some(Instant::now());
        }
    }

    pub fn finish(self, tokens: usize) {
        let ttft = self.first_token.map(|t| t - self.start).unwrap_or_default();
        let sample = LlmSample {
            ttft,
            total: self.start.elapsed(),
            tokens,
        };
        self.stats.lock().unwrap().llm.push(sample);
    }
}
