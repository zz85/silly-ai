# silly-cli

Real-time speech transcription CLI using [transcribe-rs](https://github.com/cjpais/transcribe-rs) with NVIDIA Parakeet.

## Setup

### Download the model

```bash
mkdir -p models && cd models
curl -LO https://blob.handy.computer/parakeet-v3-int8.tar.gz
tar -xzf parakeet-v3-int8.tar.gz
rm parakeet-v3-int8.tar.gz
cd ..
```

### (Optional) Download VAD model

Voice Activity Detection filters out silence, saving CPU by only transcribing when speech is detected:

```bash
cd models
curl -L -o silero_vad_v4.onnx https://github.com/cjpais/Handy/raw/refs/heads/main/src-tauri/resources/models/silero_vad_v4.onnx
cd ..
```

### Build and run

```bash
cargo run --release
```

## Features

- Real-time microphone transcription
- Preview text updates every 500ms (shown in gray)
- Voice Activity Detection (VAD) for utterance-based transcription
- Falls back to fixed 3s chunks if VAD model not found
- Automatic resampling from device sample rate to 16kHz
- Voice Activity Detection (VAD) to skip silence

## Profiling with hotpath

Install the TUI:

```bash
cargo install hotpath --features="tui"
```

Run with profiling:

```bash
cargo run --release --features hotpath
```

Or with allocation tracking:

```bash
cargo run --release --features hotpath,hotpath-alloc
```

In a separate terminal, view live metrics:

```bash
hotpath console
```
