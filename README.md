# silly-ai

"Hey Silly" is a CLI based AI voice chat assistant using real-time speech transcription, local LLMs, and TTS. It allows answering questions from LLM while completely offline, making this a good replacement for cloud based personal assistants like Siri, Alexa, Google, ChatGPT if you are ever concerned about sending anything over the internet.

## Features

- Real-time speech-to-text using [transcribe-rs](https://github.com/cjpais/transcribe-rs) with NVIDIA Parakeet
- Voice Activity Detection (VAD) with Silero for utterance segmentation
- Live preview transcription (gray text) while speaking
- **Local LLM inference** via llama.cpp with Metal GPU acceleration (or Ollama)
- **Auto-download models** from HuggingFace on first run
- Text-to-speech with [Kokoro TTS](https://huggingface.co/hexgrad/Kokoro-82M) or [Supertonic](https://github.com/supertone-inc/supertonic)
- Streaming TTS: speech starts as soon as the first sentence is generated
- Multi-threaded architecture: separate threads for audio capture, VAD, preview transcription, and final transcription
- **Hardware acceleration**: Metal on Apple Silicon for LLM, CoreML for VAD, transcription, and TTS

## Demo
[demo](https://x.com/BlurSpline/status/2004470406435295742?s=20)

## Architecture

```
┌─────────┐    ┌─────┐    ┌─────────────────┐    ┌─────────┐    ┌───────────┐    ┌─────┐
│  Audio  │───▶│ VAD │───▶│ Final Transcr.  │───▶│         │───▶│ llama.cpp │───▶│ TTS │
│ Capture │    │     │    └─────────────────┘    │ Display │    │  (Metal)  │    │     │
└─────────┘    │     │    ┌─────────────────┐    │         │    └───────────┘    └─────┘
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

### 4. (Optional) Download Kokoro TTS model and voices

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

### 5. Build and run

Start LM Studio with a model loaded, then build and run:

```bash
cargo build --release
./target/release/silly
```

### 6. (Optional) Use llama.cpp instead

For fully offline operation with auto-downloaded models:

```bash
cargo build --release --no-default-features --features supertonic,llama-cpp
```

The LLM model (TinyLlama by default) will be **automatically downloaded** from HuggingFace on first run.

### 7. (Optional) Use Ollama instead

If you prefer Ollama, start the server and build with the ollama feature:

```bash
ollama serve  # in another terminal
cargo build --release --no-default-features --features supertonic,ollama
```

### 8. Build variants

```bash
# Default (LM Studio + Supertonic TTS with CoreML)
cargo build --release

# With Kokoro TTS instead
cargo build --release --no-default-features --features lm-studio,kokoro

# With llama.cpp instead (Metal GPU, auto-downloads models)
cargo build --release --no-default-features --features supertonic,llama-cpp

# With Ollama instead
cargo build --release --no-default-features --features supertonic,ollama
```

**Note**: On Apple Silicon (M1/M2/M3), hardware acceleration is automatically enabled:
- LLM: Metal GPU via llama.cpp
- VAD: CoreML (Silero)
- TTS: CoreML (Supertonic)
- Transcription: CoreML (Parakeet)

## CLI Commands

```bash
# Full voice assistant mode (default)
silly

# Transcription-only mode (no LLM/TTS)
silly transcribe

# Test UI rendering without audio
silly test-ui [scene]  # scenes: idle, preview, thinking, speaking, response, all

# Listen mode - continuous audio capture and transcription (requires --features listen)
silly listen                      # Interactive source picker
silly listen -s mic               # Microphone input
silly listen -s system            # System audio (all apps)
silly listen -s "Safari"          # Specific app audio
silly listen --list               # List available apps
silly listen -s mic -o notes.txt  # Custom output file

# Summarize a transcription file
silly summarize -i transcript.txt
```

### Building with Listen feature

```bash
cargo build --release --features listen
```

## Usage

Say the wake word ("Hey Silly" by default) to activate, then speak your question. The CLI will:
1. Show preview text in gray while you're speaking
2. Print final transcription with `>` prefix when you pause
3. Send to LLM and stream the response in cyan
4. Speak the response using TTS (streaming sentence-by-sentence)

After responding, the assistant listens for follow-up questions for 30 seconds (configurable) before requiring the wake word again.

### Keyboard Commands

- `/mute` or `/mic` - Toggle microphone mute
- `/speak` or `/tts` - Toggle TTS output
- `/wake` - Toggle wake word requirement
- `/stats` - Show inference performance stats (transcription, TTS, LLM)
- Type text and press Enter to submit directly (bypasses transcription)

### Auto-Submit

Voice input auto-submits after 2 seconds of silence, showing a progress bar. Any typing or new speech cancels the timer.

Press `Ctrl+C` to stop.

The assistant greets you on startup when in full mode.

## Configuration

Create `config.toml` to customize (see [`config.example.toml`](config.example.toml)):

```toml
name = "Silly"
wake_word = "Hey Silly"
wake_timeout_secs = 30

[llm]
backend = "llama-cpp"
hf_repo = "TheBloke/TinyLlama-1.1B-Chat-v1.0-GGUF"
hf_file = "tinyllama-1.1b-chat-v1.0.Q4_K_M.gguf"
prompt_format = "chatml"  # chatml, mistral, or llama3

# Or use a local model:
# model_path = "models/my-model.gguf"

# Or use LM Studio:
# [llm]
# backend = "lm-studio"
# base_url = "http://localhost:1234"
# model = "default"

[tts]
engine = "supertonic"
onnx_dir = "models/supertonic/onnx"
voice_style = "models/supertonic/voice_styles/M1.json"
speed = 1.1
```

### LLM Models

| Model | Size | prompt_format | hf_repo | hf_file |
|-------|------|---------------|---------|---------|
| TinyLlama | ~670MB | chatml | `TheBloke/TinyLlama-1.1B-Chat-v1.0-GGUF` | `tinyllama-1.1b-chat-v1.0.Q4_K_M.gguf` |
| Mistral 7B | ~4GB | mistral | `TheBloke/Mistral-7B-Instruct-v0.2-GGUF` | `mistral-7b-instruct-v0.2.Q4_K_M.gguf` |
| Llama 3 8B | ~4.5GB | llama3 | `QuantFactory/Meta-Llama-3-8B-Instruct-GGUF` | `Meta-Llama-3-8B-Instruct.Q4_K_M.gguf` |

For Kokoro TTS:
```toml
[tts]
engine = "kokoro"
model = "models/kokoro-v1.0.onnx"
voices = "models/voices-v1.0.bin"
speed = 1.1
```

Other settings:
- **VAD thresholds**: Edit constants in `src/audio.rs` and `src/vad.rs`
- **Preview interval**: `PREVIEW_INTERVAL` in `src/audio.rs` (default 500ms)

## Profiling with hotpath

```bash
cargo install hotpath --features="tui"
cargo run --release --features hotpath
# In another terminal:
hotpath console
```
