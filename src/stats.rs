//! Performance stats tracking for inference operations

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Default)]
pub struct InferenceStats {
    pub transcription: Vec<Sample>,
    pub tts: Vec<Sample>,
    pub llm: Vec<Sample>,
}

#[derive(Clone)]
pub struct Sample {
    pub duration: Duration,
    pub input_size: usize,  // chars for TTS/LLM, samples for transcription
    pub output_size: usize, // chars for transcription/LLM, samples for TTS
}

impl InferenceStats {
    pub fn summary(&self) -> String {
        let mut out = String::new();
        
        if !self.transcription.is_empty() {
            let (avg, min, max, total) = Self::calc(&self.transcription);
            let avg_samples: f64 = self.transcription.iter().map(|s| s.input_size as f64).sum::<f64>() / self.transcription.len() as f64;
            let rtf = avg.as_secs_f64() / (avg_samples / 16000.0); // real-time factor
            out.push_str(&format!(
                "Transcription (n={}): avg={:.0}ms min={:.0}ms max={:.0}ms total={:.1}s RTF={:.2}x\n",
                self.transcription.len(), avg.as_millis(), min.as_millis(), max.as_millis(), total.as_secs_f64(), rtf
            ));
        }
        
        if !self.tts.is_empty() {
            let (avg, min, max, total) = Self::calc(&self.tts);
            let avg_out: f64 = self.tts.iter().map(|s| s.output_size as f64).sum::<f64>() / self.tts.len() as f64;
            let rtf = avg.as_secs_f64() / (avg_out / 24000.0); // assume 24kHz output
            out.push_str(&format!(
                "TTS (n={}): avg={:.0}ms min={:.0}ms max={:.0}ms total={:.1}s RTF={:.2}x\n",
                self.tts.len(), avg.as_millis(), min.as_millis(), max.as_millis(), total.as_secs_f64(), rtf
            ));
        }
        
        if !self.llm.is_empty() {
            let (avg, min, max, total) = Self::calc(&self.llm);
            let total_tokens: usize = self.llm.iter().map(|s| s.output_size).sum();
            let tps = total_tokens as f64 / total.as_secs_f64();
            out.push_str(&format!(
                "LLM (n={}): avg={:.0}ms min={:.0}ms max={:.0}ms total={:.1}s ~{:.1} tok/s\n",
                self.llm.len(), avg.as_millis(), min.as_millis(), max.as_millis(), total.as_secs_f64(), tps
            ));
        }
        
        if out.is_empty() {
            out.push_str("No stats recorded yet.\n");
        }
        out
    }
    
    fn calc(samples: &[Sample]) -> (Duration, Duration, Duration, Duration) {
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
    output_size: Option<usize>,
}

pub enum StatKind {
    Transcription,
    Tts,
    Llm,
}

impl<'a> Timer<'a> {
    pub fn new(stats: &'a SharedStats, kind: StatKind, input_size: usize) -> Self {
        Self { start: Instant::now(), stats, kind, input_size, output_size: None }
    }
    
    pub fn set_output_size(&mut self, size: usize) {
        self.output_size = Some(size);
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
            StatKind::Llm => stats.llm.push(sample),
        }
    }
}
