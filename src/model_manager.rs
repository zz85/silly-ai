use std::path::PathBuf;

#[cfg(not(feature = "model-download"))]
use crate::config::Config;
#[cfg(feature = "model-download")]
use crate::config::{Config, TtsConfig};
#[cfg(feature = "model-download")]
use std::path::Path;

// ============================================================================
// Model source URLs (only needed when downloading)
// ============================================================================

// VAD
#[cfg(feature = "model-download")]
const VAD_URL: &str = "https://github.com/cjpais/Handy/raw/refs/heads/main/src-tauri/resources/models/silero_vad_v4.onnx";

// Parakeet STT (tar.gz archive)
#[cfg(feature = "model-download")]
const PARAKEET_TARGZ_URL: &str = "https://blob.handy.computer/parakeet-v3-int8.tar.gz";

// Supertonic TTS (HuggingFace CDN)
#[cfg(all(feature = "model-download", feature = "supertonic"))]
const HF_SUPERTONIC_BASE: &str = "https://huggingface.co/Supertone/supertonic/resolve/main";

#[cfg(all(feature = "model-download", feature = "supertonic"))]
const SUPERTONIC_ONNX_FILES: &[&str] = &[
    "onnx/duration_predictor.onnx",
    "onnx/text_encoder.onnx",
    "onnx/vector_estimator.onnx",
    "onnx/vocoder.onnx",
    "onnx/tts.json",
    "onnx/unicode_indexer.json",
];

#[cfg(all(feature = "model-download", feature = "supertonic"))]
const SUPERTONIC_VOICE_FILES: &[&str] = &[
    "voice_styles/F1.json",
    "voice_styles/F2.json",
    "voice_styles/F3.json",
    "voice_styles/F4.json",
    "voice_styles/F5.json",
    "voice_styles/M1.json",
    "voice_styles/M2.json",
    "voice_styles/M3.json",
    "voice_styles/M4.json",
    "voice_styles/M5.json",
];

// Kokoro TTS
#[cfg(all(feature = "model-download", feature = "kokoro"))]
const KOKORO_MODEL_URL: &str = "https://github.com/thewh1teagle/kokoro-onnx/releases/download/model-files-v1.0/kokoro-v1.0.onnx";
#[cfg(all(feature = "model-download", feature = "kokoro"))]
const KOKORO_VOICES_URL: &str = "https://github.com/thewh1teagle/kokoro-onnx/releases/download/model-files-v1.0/voices-v1.0.bin";

// ============================================================================
// Public model-relative paths (used by consumers)
// ============================================================================

pub const VAD_MODEL: &str = "silero_vad_v4.onnx";
pub const PARAKEET_DIR: &str = "parakeet-tdt-0.6b-v3-int8";

// ============================================================================
// Path resolution
// ============================================================================

/// Search directories for models, in priority order.
fn search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::with_capacity(3);

    // 1. .models/ in current working directory
    dirs.push(PathBuf::from(".models"));

    // 2. ~/.local/share/silly/models/
    if let Ok(home) = std::env::var("HOME") {
        dirs.push(PathBuf::from(home).join(".local/share/silly/models"));
    }

    // 3. Legacy: models/ in current working directory
    dirs.push(PathBuf::from("models"));

    dirs
}

/// The directory where downloads are stored.
fn download_dir() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".local/share/silly/models")
    } else {
        // Fallback if HOME isn't set
        PathBuf::from(".models")
    }
}

/// Resolve a relative model path against the search chain.
///
/// Returns the first path that exists on disk, or the download directory
/// path if nothing is found (so callers get a predictable location).
pub fn resolve_model_path(relative: &str) -> PathBuf {
    for dir in search_dirs() {
        let candidate = dir.join(relative);
        if candidate.exists() {
            return candidate;
        }
    }
    // Not found anywhere -- return the download location
    download_dir().join(relative)
}

/// Check whether a model file/directory exists in any search location.
#[cfg(feature = "model-download")]
fn model_exists(relative: &str) -> bool {
    search_dirs().iter().any(|dir| dir.join(relative).exists())
}

// ============================================================================
// Download logic (behind model-download feature)
// ============================================================================

#[cfg(feature = "model-download")]
mod downloader {
    use super::*;
    use std::fs;
    use std::io::{self, Read, Write};

    /// Download a file from `url` to `dest`, creating parent directories.
    /// Prints progress to stderr.
    pub fn download_file(url: &str, dest: &Path) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }

        let filename = dest.file_name().and_then(|f| f.to_str()).unwrap_or("file");

        eprint!("  Downloading {}...", filename);

        let resp = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(600))
            .build()?
            .get(url)
            .send()?
            .error_for_status()
            .map_err(|e| format!("HTTP error for {}: {}", url, e))?;

        let total = resp.content_length();

        // Write to a temporary file first, then rename (atomic-ish)
        let tmp_dest = dest.with_extension("download");

        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            let mut file = fs::File::create(&tmp_dest)?;
            let mut reader = resp;
            let mut downloaded: u64 = 0;
            let mut buf = vec![0u8; 256 * 1024]; // 256KB chunks
            let mut last_report = std::time::Instant::now();

            loop {
                let n = reader.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                file.write_all(&buf[..n])?;
                downloaded += n as u64;

                // Report progress at most every 500ms
                if last_report.elapsed() >= std::time::Duration::from_millis(500) {
                    if let Some(total) = total {
                        let pct = (downloaded as f64 / total as f64 * 100.0) as u32;
                        let mb = downloaded as f64 / 1_048_576.0;
                        let total_mb = total as f64 / 1_048_576.0;
                        eprint!(
                            "\r  Downloading {}... {:.1}/{:.1} MB ({}%)",
                            filename, mb, total_mb, pct
                        );
                    } else {
                        let mb = downloaded as f64 / 1_048_576.0;
                        eprint!("\r  Downloading {}... {:.1} MB", filename, mb);
                    }
                    last_report = std::time::Instant::now();
                }
            }

            file.flush()?;
            Ok(())
        })();

        if let Err(e) = result {
            // Clean up partial download
            let _ = fs::remove_file(&tmp_dest);
            eprintln!(" FAILED");
            return Err(e);
        }

        // Atomic rename
        fs::rename(&tmp_dest, dest)?;

        if let Some(total) = total {
            let total_mb = total as f64 / 1_048_576.0;
            eprintln!("\r  Downloading {}... {:.1} MB - done", filename, total_mb);
        } else {
            eprintln!(" done");
        }

        Ok(())
    }

    /// Download a tar.gz archive and extract it to `dest_dir`.
    pub fn download_and_extract_targz(
        url: &str,
        dest_dir: &Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
        use flate2::read::GzDecoder;
        use tar::Archive;

        fs::create_dir_all(dest_dir)?;

        eprintln!("  Downloading and extracting Parakeet STT model...");

        let resp = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(600))
            .build()?
            .get(url)
            .send()?
            .error_for_status()
            .map_err(|e| format!("HTTP error for {}: {}", url, e))?;

        let total = resp.content_length();
        if let Some(total) = total {
            let mb = total as f64 / 1_048_576.0;
            eprintln!("  Archive size: {:.1} MB", mb);
        }

        // Stream through gzip decoder and tar extractor
        let progress_reader = ProgressReader::new(resp, total);
        let gz = GzDecoder::new(progress_reader);
        let mut archive = Archive::new(gz);

        archive.unpack(dest_dir)?;

        eprintln!("  Parakeet STT model extracted");

        Ok(())
    }

    /// Wraps a Read impl and reports progress to stderr.
    struct ProgressReader<R: Read> {
        inner: R,
        downloaded: u64,
        total: Option<u64>,
        last_report: std::time::Instant,
    }

    impl<R: Read> ProgressReader<R> {
        fn new(inner: R, total: Option<u64>) -> Self {
            Self {
                inner,
                downloaded: 0,
                total,
                last_report: std::time::Instant::now(),
            }
        }
    }

    impl<R: Read> Read for ProgressReader<R> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let n = self.inner.read(buf)?;
            self.downloaded += n as u64;

            if self.last_report.elapsed() >= std::time::Duration::from_millis(500) {
                if let Some(total) = self.total {
                    let pct = (self.downloaded as f64 / total as f64 * 100.0) as u32;
                    let mb = self.downloaded as f64 / 1_048_576.0;
                    eprint!(
                        "\r  Downloading parakeet-v3-int8.tar.gz... {:.1} MB ({}%)",
                        mb, pct
                    );
                }
                self.last_report = std::time::Instant::now();
            }

            Ok(n)
        }
    }

    type DownloadFn = Box<dyn Fn(&Path) -> Result<(), Box<dyn std::error::Error>>>;

    /// Ensure all required models are present. Downloads missing ones.
    ///
    /// Returns the base model directory path.
    pub fn ensure_models(config: &Config) -> Result<PathBuf, Box<dyn std::error::Error>> {
        let base = download_dir();
        let mut needed: Vec<(&str, DownloadFn)> = Vec::new();

        // --- VAD ---
        if !model_exists(VAD_MODEL) {
            needed.push((
                "Silero VAD v4",
                Box::new(|base: &Path| download_file(VAD_URL, &base.join(VAD_MODEL))),
            ));
        }

        // --- Parakeet STT ---
        // Check for the directory with at least the encoder model
        if !model_exists(&format!("{}/encoder-model.int8.onnx", PARAKEET_DIR)) {
            needed.push((
                "Parakeet STT (int8)",
                Box::new(|base: &Path| download_and_extract_targz(PARAKEET_TARGZ_URL, base)),
            ));
        }

        // --- TTS models (based on config) ---
        match &config.tts {
            #[cfg(feature = "supertonic")]
            TtsConfig::Supertonic { .. } => {
                // Check if any supertonic ONNX file is missing
                let onnx_missing = SUPERTONIC_ONNX_FILES
                    .iter()
                    .any(|f| !model_exists(&format!("supertonic/{}", f)));
                let voices_missing = SUPERTONIC_VOICE_FILES
                    .iter()
                    .any(|f| !model_exists(&format!("supertonic/{}", f)));

                if onnx_missing {
                    needed.push((
                        "Supertonic TTS models",
                        Box::new(|base: &Path| {
                            for file in SUPERTONIC_ONNX_FILES {
                                let url = format!("{}/{}", HF_SUPERTONIC_BASE, file);
                                let dest = base.join("supertonic").join(file);
                                download_file(&url, &dest)?;
                            }
                            Ok(())
                        }),
                    ));
                }

                if voices_missing {
                    needed.push((
                        "Supertonic voice styles",
                        Box::new(|base: &Path| {
                            for file in SUPERTONIC_VOICE_FILES {
                                let url = format!("{}/{}", HF_SUPERTONIC_BASE, file);
                                let dest = base.join("supertonic").join(file);
                                download_file(&url, &dest)?;
                            }
                            Ok(())
                        }),
                    ));
                }
            }
            #[cfg(feature = "kokoro")]
            TtsConfig::Kokoro { .. } => {
                if !model_exists("kokoro-v1.0.onnx") {
                    needed.push((
                        "Kokoro TTS model",
                        Box::new(|base: &Path| {
                            download_file(KOKORO_MODEL_URL, &base.join("kokoro-v1.0.onnx"))
                        }),
                    ));
                }
                if !model_exists("voices-v1.0.bin") {
                    needed.push((
                        "Kokoro voice embeddings",
                        Box::new(|base: &Path| {
                            download_file(KOKORO_VOICES_URL, &base.join("voices-v1.0.bin"))
                        }),
                    ));
                }
            }
            // If feature not enabled, config parsing would have caught it
            #[allow(unreachable_patterns)]
            _ => {}
        }

        if needed.is_empty() {
            return Ok(base);
        }

        eprintln!(
            "\nDownloading {} missing model(s) to {}...\n",
            needed.len(),
            base.display(),
        );

        std::fs::create_dir_all(&base)?;

        let mut failures = Vec::new();

        for (name, download_fn) in &needed {
            eprintln!("[{}]", name);
            if let Err(e) = download_fn(&base) {
                eprintln!("  ERROR: {}", e);
                failures.push(*name);
            }
            eprintln!();
        }

        if !failures.is_empty() {
            eprintln!(
                "Warning: Failed to download {} model(s): {}",
                failures.len(),
                failures.join(", ")
            );
            eprintln!("The application may not function correctly.");
            eprintln!("See README.md for manual download instructions.\n");
        } else {
            eprintln!("All models ready.\n");
        }

        Ok(base)
    }
}

// ============================================================================
// Public API
// ============================================================================

/// Ensure all required models are present, downloading if needed.
///
/// When the `model-download` feature is enabled, this will automatically
/// download any missing models. Otherwise it just resolves the model directory.
///
/// Returns the base model directory path.
#[cfg(feature = "model-download")]
pub fn ensure_models(config: &Config) -> Result<PathBuf, Box<dyn std::error::Error>> {
    downloader::ensure_models(config)
}

/// When model-download is not enabled, just return the best model directory.
#[cfg(not(feature = "model-download"))]
pub fn ensure_models(_config: &Config) -> Result<PathBuf, Box<dyn std::error::Error>> {
    // Return the first search dir that exists, or the download dir
    for dir in search_dirs() {
        if dir.exists() {
            return Ok(dir);
        }
    }
    Ok(download_dir())
}
