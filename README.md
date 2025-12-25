# silly-cli

Voice-first AI chat CLI using real-time speech transcription and Ollama.

## Features

- Real-time speech-to-text using [transcribe-rs](https://github.com/cjpais/transcribe-rs) with NVIDIA Parakeet
- Voice Activity Detection (VAD) with Silero for utterance segmentation
- Live preview transcription (gray text) while speaking
- Conversational AI via Ollama with voice-optimized system prompt
- Multi-threaded architecture: separate threads for audio capture, VAD, preview transcription, and final transcription

## Architecture

```
┌─────────┐    ┌─────┐    ┌─────────────────┐    ┌─────────┐
│  Audio  │───▶│ VAD │───▶│ Final Transcr.  │───▶│         │
│ Capture │    │     │    └─────────────────┘    │ Display │───▶ Ollama Chat
└─────────┘    │     │    ┌─────────────────┐    │         │
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

### 2. Download VAD model (recommended)

```bash
cd models
curl -L -o silero_vad_v4.onnx https://github.com/cjpais/Handy/raw/refs/heads/main/src-tauri/resources/models/silero_vad_v4.onnx
cd ..
```

### 3. Start Ollama

```bash
ollama serve
```

Make sure you have a model available (default: `gpt-oss:20b`). Edit `src/chat.rs` to change the model.

### 4. Build and run

```bash
cargo run --release
```

## Usage

Just speak! The CLI will:
1. Show preview text in gray while you're speaking
2. Print final transcription with `>` prefix when you pause
3. Send to Ollama and display the response in cyan

Press `Ctrl+C` to stop.

## Configuration

- **Model**: Edit `MODEL` constant in `src/chat.rs`
- **VAD thresholds**: Edit constants in `src/audio.rs` and `src/vad.rs`
- **Preview interval**: `PREVIEW_INTERVAL` in `src/audio.rs` (default 500ms)

## Profiling with hotpath

```bash
cargo install hotpath --features="tui"
cargo run --release --features hotpath
# In another terminal:
hotpath console
```
