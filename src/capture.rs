use flume::Sender;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub const TARGET_RATE: usize = 16000;
const CAPTURE_SAMPLE_RATE: usize = 48000;

pub fn resample(samples: &[f32], from_rate: usize, to_rate: usize) -> Vec<f32> {
    if from_rate == to_rate {
        return samples.to_vec();
    }
    let ratio = to_rate as f64 / from_rate as f64;
    let new_len = (samples.len() as f64 * ratio) as usize;
    (0..new_len)
        .map(|i| {
            let src_idx = i as f64 / ratio;
            let idx = src_idx as usize;
            let frac = src_idx - idx as f64;
            if idx + 1 < samples.len() {
                samples[idx] * (1.0 - frac as f32) + samples[idx + 1] * frac as f32
            } else {
                samples.get(idx).copied().unwrap_or(0.0)
            }
        })
        .collect()
}

pub fn capture_mic(
    tx: Sender<Vec<f32>>,
    running: Arc<AtomicBool>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

    let host = cpal::default_host();
    let device = host.default_input_device().ok_or("No input device")?;
    let supported = device.default_input_config()?;
    let sample_rate = u32::from(supported.sample_rate()) as usize;
    let channels = supported.channels() as usize;

    println!("Mic: {}Hz {}ch", sample_rate, channels);

    let stream = device.build_input_stream(
        &supported.config(),
        move |data: &[f32], _| {
            let mono: Vec<f32> = if channels == 1 {
                data.to_vec()
            } else {
                data.chunks(channels)
                    .map(|c| c.iter().sum::<f32>() / channels as f32)
                    .collect()
            };
            let resampled = resample(&mono, sample_rate, TARGET_RATE);
            let _ = tx.send(resampled);
        },
        |e| eprintln!("Mic error: {}", e),
        None,
    )?;
    stream.play()?;

    while running.load(Ordering::SeqCst) {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    Ok(())
}

pub fn capture_system(
    tx: Sender<Vec<f32>>,
    running: Arc<AtomicBool>,
    app_filter: Option<String>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use screencapturekit::prelude::*;

    let content = SCShareableContent::get()?;
    let display = content.displays().into_iter().next().ok_or("No display")?;

    let filter = if let Some(name) = &app_filter {
        let name_lower = name.to_lowercase();
        let app = content
            .applications()
            .into_iter()
            .find(|a| a.application_name().to_lowercase().contains(&name_lower))
            .ok_or_else(|| format!("App '{}' not found", name))?;
        println!("Capturing: {}", app.application_name());
        SCContentFilter::create()
            .with_display(&display)
            .with_including_applications(&[&app], &[])
            .build()
    } else {
        println!("Capturing: system audio");
        SCContentFilter::create()
            .with_display(&display)
            .with_excluding_windows(&[])
            .build()
    };

    let config = SCStreamConfiguration::new()
        .with_width(2)
        .with_height(2)
        .with_captures_audio(true)
        .with_sample_rate(CAPTURE_SAMPLE_RATE as i32)
        .with_channel_count(1);

    let mut stream = SCStream::new(&filter, &config);

    stream.add_output_handler(
        move |sample: CMSampleBuffer, of_type: SCStreamOutputType| {
            if !matches!(of_type, SCStreamOutputType::Audio) {
                return;
            }
            if let Some(audio_buffers) = sample.audio_buffer_list() {
                for buf in &audio_buffers {
                    let bytes = buf.data();
                    if bytes.is_empty() {
                        continue;
                    }
                    let samples: Vec<f32> = bytes
                        .chunks_exact(4)
                        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
                        .collect();
                    let resampled = resample(&samples, CAPTURE_SAMPLE_RATE, TARGET_RATE);
                    let _ = tx.send(resampled);
                }
            }
        },
        SCStreamOutputType::Audio,
    );

    stream.start_capture()?;

    while running.load(Ordering::SeqCst) {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    let _ = stream.stop_capture();
    Ok(())
}

pub fn list_apps() -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
    use screencapturekit::prelude::*;
    let content = SCShareableContent::get()?;
    Ok(content
        .applications()
        .into_iter()
        .map(|a| a.application_name().to_string())
        .collect())
}
