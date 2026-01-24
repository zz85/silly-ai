//! Acoustic Echo Cancellation using aec3 crate
//!
//! When enabled, processes mic input to remove TTS audio echo.
//! AEC runs on the VAD thread since VoipAec3 is not Send.

use aec3::voip::VoipAec3;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};

const SAMPLE_RATE: usize = 16000;
const CHANNELS: usize = 1;

/// AEC processor that runs on a single thread
pub struct AecProcessor {
    inner: VoipAec3,
    frame_samples: usize,
    render_rx: Receiver<Vec<f32>>,
}

impl AecProcessor {
    pub fn new(render_rx: Receiver<Vec<f32>>) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let pipeline = VoipAec3::builder(SAMPLE_RATE, CHANNELS, CHANNELS)
            .enable_high_pass(true)
            .build()?;
        let frame_samples = pipeline.capture_frame_samples();
        Ok(Self {
            inner: pipeline,
            frame_samples,
            render_rx,
        })
    }

    /// Drain any pending render frames and feed them to AEC
    fn drain_render(&mut self) {
        loop {
            match self.render_rx.try_recv() {
                Ok(samples) => {
                    for chunk in samples.chunks(self.frame_samples) {
                        if chunk.len() == self.frame_samples {
                            let _ = self.inner.handle_render_frame(chunk);
                        }
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            }
        }
    }

    /// Process mic audio (capture/near-end) and return echo-cancelled output
    pub fn process_capture(&mut self, samples: &[f32]) -> Vec<f32> {
        // First drain any pending render frames
        self.drain_render();

        let mut out = vec![0.0f32; samples.len()];
        for (i, chunk) in samples.chunks(self.frame_samples).enumerate() {
            if chunk.len() == self.frame_samples {
                let start = i * self.frame_samples;
                let _ = self.inner.process_capture_frame(
                    chunk,
                    false,
                    &mut out[start..start + self.frame_samples],
                );
            }
        }
        out
    }
}

/// Channel sender for feeding TTS audio to AEC
pub type AecRenderTx = Sender<Vec<f32>>;
