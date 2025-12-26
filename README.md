# silly-ai

"Hey Silly" is a voice-first AI chat CLI using real-time speech transcription, Ollama, and TTS. It allows answering questions from LLM while completely offline, making this a good replacement for cloud based personal assistants like Siri, Alexa, Google, ChatGPT if you are ever concerned about sending anything over the internet.

## Features

- Real-time speech-to-text using [transcribe-rs](https://github.com/cjpais/transcribe-rs) with NVIDIA Parakeet
- Voice Activity Detection (VAD) with Silero for utterance segmentation
- Live preview transcription (gray text) while speaking
- Conversational AI via Ollama with voice-optimized system prompt
- Text-to-speech with [Kokoro TTS](https://huggingface.co/hexgrad/Kokoro-82M) or [Supertonic](https://github.com/supertone-inc/supertonic)
- Streaming TTS: speech starts as soon as the first sentence is generated
- Multi-threaded architecture: separate threads for audio capture, VAD, preview transcription, and final transcription

## Architecture

```
┌─────────┐    ┌─────┐    ┌─────────────────┐    ┌─────────┐    ┌────────┐    ┌─────┐
│  Audio  │───▶│ VAD │───▶│ Final Transcr.  │───▶│         │───▶│ Ollama │───▶│ TTS │
│ Capture │    │     │    └─────────────────┘    │ Display │    │  Chat  │    │     │
└─────────┘    │     │    ┌─────────────────┐    │         │    └────────┘    └─────┘
               │     │───▶│Preview Transcr. │───▶│         │
               └─────┘    └─────────────────┘    └─────────┘
                          (lossy channel)
```

## Setup

### 1. Download the Parakeet model

```bash
mkdir -p models && cd models
curl -LO https://blob.handy.computer/parakeet-v3-int8.tar.gz
tar -xzf parakeet-v3-int8.tar.gz
rm parakeet-v3-int8.tar.gz
cd ..
```

### 2. Download VAD model

```bash
cd models
curl -L -o silero_vad_v4.onnx https://github.com/cjpais/Handy/raw/refs/heads/main/src-tauri/resources/models/silero_vad_v4.onnx
cd ..
```

### 3. Download Supertonic TTS models (default)

```bash
brew install git-lfs && git lfs install
cd models && mkdir -p supertonic && cd supertonic
git clone --depth 1 https://huggingface.co/Supertone/supertonic assets
cd assets && git lfs pull && cd ../..
```

### 4b. (Optional) Download Kokoro TTS model and voices

#### system dependencies (macOS)

```bash
brew install cmake  # Required for espeak-rs-sys
```

```bash
cd models
curl -L "https://github.com/thewh1teagle/kokoro-onnx/releases/download/model-files-v1.0/kokoro-v1.0.onnx" -o kokoro-v1.0.onnx
curl -L "https://github.com/thewh1teagle/kokoro-onnx/releases/download/model-files-v1.0/voices-v1.0.bin" -o voices-v1.0.bin
cd ..
```

### 5. Start Ollama

```bash
ollama serve
```

Make sure you have a model available (default: `gpt-oss:20b`). Edit `src/chat.rs` to change the model.

### 6. Build and run

```bash
# Default (Supertonic TTS)
cargo build --release
./target/release/silly

# With Kokoro TTS instead
AUDIOPUS_SYS_USE_PKG_CONFIG=1 cargo build --release --no-default-features --features kokoro

# With both TTS engines
cargo build --release --features kokoro
```

## Usage

Say the wake word ("Hey Silly" by default) to activate, then speak your question. The CLI will:
1. Show preview text in gray while you're speaking
2. Print final transcription with `>` prefix when you pause
3. Send to Ollama and stream the response in cyan
4. Speak the response using TTS (streaming sentence-by-sentence)

After responding, the assistant listens for follow-up questions for 30 seconds (configurable) before requiring the wake word again.

Press `Ctrl+C` to stop.

## Configuration

Create `config.toml` to customize (see [`config.example.toml`](config.example.toml)):

```toml
name = "Silly"
wake_word = "Hey Silly"
wake_timeout_secs = 30

[tts]
engine = "supertonic"
onnx_dir = "models/supertonic/assets/onnx"
voice_style = "models/supertonic/assets/voice_styles/F2.json"
```

Or for Kokoro TTS:
```toml
[tts]
engine = "kokoro"
model = "models/kokoro-v1.0.onnx"
voices = "models/voices-v1.0.bin"
```

Other settings:
- **LLM Model**: Edit `MODEL` constant in `src/chat.rs` (default: `gpt-oss:20b`)
- **VAD thresholds**: Edit constants in `src/audio.rs` and `src/vad.rs`
- **Preview interval**: `PREVIEW_INTERVAL` in `src/audio.rs` (default 500ms)

## Profiling with hotpath

```bash
cargo install hotpath --features="tui"
AUDIOPUS_SYS_USE_PKG_CONFIG=1 cargo run --release --features hotpath
# In another terminal:
hotpath console
```
