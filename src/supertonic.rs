// Vendored from https://github.com/supertone-inc/supertonic (MIT License)
// Minimal subset for TTS inference

use ndarray::{Array, Array3};
use ort::{session::Session, value::Value};
use ort::execution_providers::CoreMLExecutionProvider;
use rand_distr::{Distribution, Normal};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use unicode_normalization::UnicodeNormalization;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub ae: AEConfig,
    pub ttl: TTLConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AEConfig {
    pub sample_rate: i32,
    pub base_chunk_size: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TTLConfig {
    pub chunk_compress_factor: i32,
    pub latent_dim: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceStyleData {
    pub style_ttl: StyleComponent,
    pub style_dp: StyleComponent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StyleComponent {
    pub data: Vec<Vec<Vec<f32>>>,
    pub dims: Vec<usize>,
    #[serde(rename = "type")]
    pub dtype: String,
}

pub struct Style {
    pub ttl: Array3<f32>,
    pub dp: Array3<f32>,
}

pub struct UnicodeProcessor {
    indexer: Vec<i64>,
}

impl UnicodeProcessor {
    pub fn new<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let file = File::open(path)?;
        let indexer: Vec<i64> = serde_json::from_reader(BufReader::new(file))?;
        Ok(Self { indexer })
    }

    pub fn call(&self, text_list: &[String]) -> (Vec<Vec<i64>>, Array3<f32>) {
        let processed: Vec<String> = text_list.iter().map(|t| preprocess_text(t)).collect();
        let lengths: Vec<usize> = processed.iter().map(|t| t.chars().count()).collect();
        let max_len = *lengths.iter().max().unwrap_or(&0);

        let mut text_ids = Vec::new();
        for text in &processed {
            let mut row = vec![0i64; max_len];
            for (j, c) in text.chars().enumerate() {
                let val = c as usize;
                row[j] = if val < self.indexer.len() {
                    self.indexer[val]
                } else {
                    -1
                };
            }
            text_ids.push(row);
        }

        let text_mask = length_to_mask(&lengths, Some(max_len));
        (text_ids, text_mask)
    }
}

fn preprocess_text(text: &str) -> String {
    let mut text: String = text.nfkd().collect();

    // Remove emojis
    let emoji_re = Regex::new(r"[\x{1F600}-\x{1F64F}\x{1F300}-\x{1F5FF}\x{1F680}-\x{1F6FF}\x{2600}-\x{26FF}\x{2700}-\x{27BF}]+").unwrap();
    text = emoji_re.replace_all(&text, "").to_string();

    // Basic replacements
    for (from, to) in [
        ("–", "-"),
        ("—", "-"),
        ("_", " "),
        ("\u{201C}", "\""),
        ("\u{201D}", "\""),
        ("\u{2018}", "'"),
        ("\u{2019}", "'"),
    ] {
        text = text.replace(from, to);
    }

    // Clean whitespace
    text = Regex::new(r"\s+")
        .unwrap()
        .replace_all(&text, " ")
        .trim()
        .to_string();

    // Add period if missing
    if !text.is_empty() && !text.ends_with(|c| ".!?;:".contains(c)) {
        text.push('.');
    }
    text
}

fn length_to_mask(lengths: &[usize], max_len: Option<usize>) -> Array3<f32> {
    let bsz = lengths.len();
    let max_len = max_len.unwrap_or_else(|| *lengths.iter().max().unwrap_or(&0));
    let mut mask = Array3::<f32>::zeros((bsz, 1, max_len));
    for (i, &len) in lengths.iter().enumerate() {
        for j in 0..len.min(max_len) {
            mask[[i, 0, j]] = 1.0;
        }
    }
    mask
}

fn sample_noisy_latent(
    duration: &[f32],
    sample_rate: i32,
    base_chunk_size: i32,
    chunk_compress: i32,
    latent_dim: i32,
) -> (Array3<f32>, Array3<f32>) {
    let bsz = duration.len();
    let max_dur = duration.iter().fold(0.0f32, |a, &b| a.max(b));
    let wav_len_max = (max_dur * sample_rate as f32) as usize;
    let wav_lengths: Vec<usize> = duration
        .iter()
        .map(|&d| (d * sample_rate as f32) as usize)
        .collect();

    let chunk_size = (base_chunk_size * chunk_compress) as usize;
    let latent_len = (wav_len_max + chunk_size - 1) / chunk_size;
    let latent_dim_val = (latent_dim * chunk_compress) as usize;

    let mut noisy_latent = Array3::<f32>::zeros((bsz, latent_dim_val, latent_len));
    let normal = Normal::new(0.0, 1.0).unwrap();
    let mut rng = rand::thread_rng();

    for b in 0..bsz {
        for d in 0..latent_dim_val {
            for t in 0..latent_len {
                noisy_latent[[b, d, t]] = normal.sample(&mut rng);
            }
        }
    }

    let latent_lengths: Vec<usize> = wav_lengths
        .iter()
        .map(|&len| (len + chunk_size - 1) / chunk_size)
        .collect();
    let latent_mask = length_to_mask(&latent_lengths, Some(latent_len));

    for b in 0..bsz {
        for d in 0..latent_dim_val {
            for t in 0..latent_len {
                noisy_latent[[b, d, t]] *= latent_mask[[b, 0, t]];
            }
        }
    }

    (noisy_latent, latent_mask)
}

pub struct TextToSpeech {
    cfgs: Config,
    text_processor: UnicodeProcessor,
    dp_ort: Session,
    text_enc_ort: Session,
    vector_est_ort: Session,
    vocoder_ort: Session,
    pub sample_rate: i32,
}

impl TextToSpeech {
    pub fn call(
        &mut self,
        text: &str,
        style: &Style,
        total_step: usize,
        speed: f32,
        _silence_duration: f32,
    ) -> anyhow::Result<(Vec<f32>, f32)> {
        let text_list = vec![text.to_string()];
        let bsz = 1;

        let (text_ids, text_mask) = self.text_processor.call(&text_list);
        let text_ids_array = Array::from_shape_vec(
            (bsz, text_ids[0].len()),
            text_ids.into_iter().flatten().collect(),
        )?;

        let text_ids_value = Value::from_array(text_ids_array)?;
        let text_mask_value = Value::from_array(text_mask.clone())?;
        let style_dp_value = Value::from_array(style.dp.clone())?;

        // Duration prediction
        let dp_outputs = self.dp_ort.run(ort::inputs!{ "text_ids" => &text_ids_value, "style_dp" => &style_dp_value, "text_mask" => &text_mask_value })?;
        let (_, duration_data) = dp_outputs["duration"].try_extract_tensor::<f32>()?;
        let duration: f32 = duration_data.iter().next().copied().unwrap_or(1.0) / speed;

        // Text encoding
        let style_ttl_value = Value::from_array(style.ttl.clone())?;
        let text_enc_outputs = self.text_enc_ort.run(ort::inputs!{ "text_ids" => &text_ids_value, "style_ttl" => &style_ttl_value, "text_mask" => &text_mask_value })?;
        let (text_emb_shape, text_emb_data) =
            text_enc_outputs["text_emb"].try_extract_tensor::<f32>()?;
        let text_emb = Array3::from_shape_vec(
            (
                text_emb_shape[0] as usize,
                text_emb_shape[1] as usize,
                text_emb_shape[2] as usize,
            ),
            text_emb_data.to_vec(),
        )?;

        // Sample noisy latent
        let (mut xt, latent_mask) = sample_noisy_latent(
            &[duration],
            self.sample_rate,
            self.cfgs.ae.base_chunk_size,
            self.cfgs.ttl.chunk_compress_factor,
            self.cfgs.ttl.latent_dim,
        );

        // Denoising loop
        let total_step_array = Array::from_elem(bsz, total_step as f32);
        for step in 0..total_step {
            let current_step_array = Array::from_elem(bsz, step as f32);
            let outputs = self.vector_est_ort.run(ort::inputs! {
                "noisy_latent" => Value::from_array(xt.clone())?,
                "text_emb" => Value::from_array(text_emb.clone())?,
                "style_ttl" => &style_ttl_value,
                "latent_mask" => Value::from_array(latent_mask.clone())?,
                "text_mask" => Value::from_array(text_mask.clone())?,
                "current_step" => Value::from_array(current_step_array)?,
                "total_step" => Value::from_array(total_step_array.clone())?
            })?;
            let (shape, data) = outputs["denoised_latent"].try_extract_tensor::<f32>()?;
            xt = Array3::from_shape_vec(
                (shape[0] as usize, shape[1] as usize, shape[2] as usize),
                data.to_vec(),
            )?;
        }

        // Vocoder
        let vocoder_outputs = self
            .vocoder_ort
            .run(ort::inputs! { "latent" => Value::from_array(xt)? })?;
        let (_, wav_data) = vocoder_outputs["wav_tts"].try_extract_tensor::<f32>()?;
        let wav: Vec<f32> = wav_data.to_vec();
        let wav_len = ((self.sample_rate as f32 * duration) as usize).min(wav.len());

        Ok((wav[..wav_len].to_vec(), duration))
    }
}

pub fn load_text_to_speech<P: AsRef<Path>>(
    onnx_dir: P,
    use_gpu: bool,
) -> anyhow::Result<TextToSpeech> {
    let onnx_dir = onnx_dir.as_ref();

    let cfg_file = File::open(onnx_dir.join("tts.json"))?;
    let cfgs: Config = serde_json::from_reader(BufReader::new(cfg_file))?;

    let text_processor = UnicodeProcessor::new(onnx_dir.join("unicode_indexer.json"))?;

    #[cfg(all(target_arch = "aarch64", target_os = "macos"))]
    let create_session = |model_path: &std::path::PathBuf, model_name: &str| -> ort::Result<Session> {
        if use_gpu {
            println!("Loading {} with CoreML...", model_name);
            match Session::builder()?
                .with_execution_providers([
                    CoreMLExecutionProvider::default()
                        .with_subgraphs(true)
                        .build()
                ]) {
                Ok(builder) => builder.commit_from_file(model_path),
                Err(e) => {
                    eprintln!("CoreML EP failed for {}, falling back to CPU: {}", model_name, e);
                    Session::builder()?.commit_from_file(model_path)
                }
            }
        } else {
            Session::builder()?.commit_from_file(model_path)
        }
    };

    #[cfg(not(all(target_arch = "aarch64", target_os = "macos")))]
    let create_session = |model_path: &std::path::PathBuf, _model_name: &str| -> ort::Result<Session> {
        Session::builder()?.commit_from_file(model_path)
    };

    let dp_ort = create_session(&onnx_dir.join("duration_predictor.onnx"), "duration_predictor")?;
    let text_enc_ort = create_session(&onnx_dir.join("text_encoder.onnx"), "text_encoder")?;
    let vector_est_ort = create_session(&onnx_dir.join("vector_estimator.onnx"), "vector_estimator")?;
    let vocoder_ort = create_session(&onnx_dir.join("vocoder.onnx"), "vocoder")?;

    let sample_rate = cfgs.ae.sample_rate;
    Ok(TextToSpeech {
        cfgs,
        text_processor,
        dp_ort,
        text_enc_ort,
        vector_est_ort,
        vocoder_ort,
        sample_rate,
    })
}

pub fn load_voice_style(paths: &[String], _verbose: bool) -> anyhow::Result<Style> {
    let file = File::open(&paths[0])?;
    let data: VoiceStyleData = serde_json::from_reader(BufReader::new(file))?;

    let ttl_flat: Vec<f32> = data
        .style_ttl
        .data
        .into_iter()
        .flatten()
        .flatten()
        .collect();
    let dp_flat: Vec<f32> = data.style_dp.data.into_iter().flatten().flatten().collect();

    let ttl = Array3::from_shape_vec(
        (
            data.style_ttl.dims[0],
            data.style_ttl.dims[1],
            data.style_ttl.dims[2],
        ),
        ttl_flat,
    )?;
    let dp = Array3::from_shape_vec(
        (
            data.style_dp.dims[0],
            data.style_dp.dims[1],
            data.style_dp.dims[2],
        ),
        dp_flat,
    )?;

    Ok(Style { ttl, dp })
}
