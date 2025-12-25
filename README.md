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

### Build and run

```bash
cargo run --release
```

## Features

- Real-time microphone transcription
- Preview text updates every 500ms (shown in gray)
- Final transcription every 3 seconds
- Automatic resampling from device sample rate to 16kHz

## Profiling with hotpath

Install the TUI:

```bash
cargo install hotpath --features="tui"
```

Run with profiling (and allocation tracking):

```bash
cargo run --release --features hotpath,hotpath-alloc
```

In a separate terminal, view live metrics:

```bash
hotpath console
```
