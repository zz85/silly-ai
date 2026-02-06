# Silly AI

This is my playground of local AI LLM experiments. It's super useful when you want AI capabilities while offline. Even if you have internet, local LLMs avoids transmitting data to a 3rd-party.

- `silly` - "Hey Silly" is a CLI based AI voice chat assistant using real-time speech transcription, local LLMs, and TTS. It allows answering questions from LLM while completely offline, making this a good replacement for cloud based personal assistants like Siri, Alexa, Google, ChatGPT if you are ever concerned about sending anything over the internet
- `silly transcribe` - Converts ogg/wav files into text
- `silly orb-demo` - Interactive demo of graphical orb animations (cycles through all states and styles)
- `silly summarize` - Summarizes transcripts
- `ai department` - A department/roundtable of "experts"

## Features

- Real-time speech-to-text using [transcribe-rs](https://github.com/cjpais/transcribe-rs) with NVIDIA Parakeet
- Voice Activity Detection (VAD) with Silero for utterance segmentation
- Live preview transcription (gray text) while speaking
- **Local LLM inference** via llama.cpp with Metal GPU acceleration (or Ollama)
- **Auto-download models** from HuggingFace on first run
- Text-to-speech with [Kokoro TTS](https://huggingface.co/hexgrad/Kokoro-82M) or [Supertonic](https://github.com/supertone-inc/supertonic)
- Streaming TTS: speech starts as soon as the first sentence is generated
- **Real-time audio visualization**: Animated bars showing microphone input and TTS output volume levels
- Multi-threaded architecture: separate threads for audio capture, VAD, preview transcription, and final transcription
- **Hardware acceleration**: Metal on Apple Silicon for LLM, CoreML for VAD, transcription, and TTS
- **Crosstalk mode**: Continue listening while TTS plays, with barge-in support
- **Multiple modes**: Chat, Transcribe, and Note-taking modes

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

### Crosstalk Flow

When crosstalk is enabled, the system continues processing audio while TTS is playing:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           Crosstalk System                                   │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│   ┌─────────┐    ┌─────┐    ┌──────────────────────┐                        │
│   │  Mic    │───▶│ VAD │───▶│ Crosstalk Enabled?   │                        │
│   │ Input   │    │     │    └──────────┬───────────┘                        │
│   └─────────┘    └─────┘               │                                    │
│                                        │                                    │
│                    ┌───────────────────┴───────────────────┐                │
│                    │                                       │                │
│                    ▼                                       ▼                │
│            ┌──────────────┐                       ┌──────────────┐          │
│            │     Yes      │                       │      No      │          │
│            │ Continue     │                       │ Mute during  │          │
│            │ Processing   │                       │    TTS       │          │
│            └──────┬───────┘                       └──────────────┘          │
│                   │                                                         │
│                   ▼                                                         │
│         ┌─────────────────┐                                                 │
│         │ Speech Detected │                                                 │
│         │  during TTS?    │                                                 │
│         └────────┬────────┘                                                 │
│                  │                                                          │
│         ┌────────┴────────┐                                                 │
│         │                 │                                                 │
│         ▼                 ▼                                                 │
│   ┌───────────┐    ┌───────────┐                                           │
│   │ Duck TTS  │    │ Barge-in: │                                           │
│   │ to 20%    │    │ Stop TTS  │                                           │
│   │ volume    │    │ + Process │                                           │
│   └───────────┘    │ new input │                                           │
│                    └───────────┘                                           │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘
```

**Crosstalk behaviors:**
- **Volume ducking**: When you speak while TTS is playing, volume reduces to 20%
- **Barge-in**: Your speech stops TTS and processes your new input
- **Stop command**: Say "stop" to halt TTS without triggering a new response

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

### 5. Configure your LLM backend

Create or edit `config.toml`:

**Option A: Use LM Studio (OpenAI-compatible API)**
```toml
[llm]
backend = "openai-compat"
preset = "lm_studio"
model = "your-model-name"
```

Start LM Studio with a model loaded.

**Option B: Use llama.cpp (fully offline)**
```toml
[llm]
backend = "llama-cpp"
hf_repo = "TheBloke/TinyLlama-1.1B-Chat-v1.0-GGUF"
hf_file = "tinyllama-1.1b-chat-v1.0.Q4_K_M.gguf"
prompt_format = "chatml"
```

**Option C: Use Ollama**
```toml
[llm]
backend = "ollama"
model = "mistral:7b-instruct"
```

Start Ollama server: `ollama serve`

### 6. Build and run

```bash
cargo build --release
./target/release/silly
```

### 7. Build variants

```bash
# Default (OpenAI-compatible API + Supertonic TTS with CoreML)
cargo build --release

# With Kokoro TTS instead
cargo build --release --no-default-features --features openai-compat,kokoro

# With llama.cpp instead (Metal GPU, auto-downloads models)
cargo build --release --no-default-features --features supertonic,llama-cpp

# With Ollama instead
cargo build --release --no-default-features --features supertonic,ollama

# With acoustic echo cancellation (AEC)
cargo build --release --features aec

# With voice-to-keyboard typing
cargo build --release --features typing
```

**Note**: On Apple Silicon (M1/M2/M3), hardware acceleration is automatically enabled:
- LLM: Metal GPU via llama.cpp or CoreML
- VAD: CoreML (Silero)
- TTS: CoreML (Supertonic)
- Transcription: CoreML (Parakeet)

## CLI Commands

```bash
# Full voice assistant mode (default)
silly

# Transcription-only mode (no LLM/TTS)
silly transcribe

# Quick LLM test
silly probe "What is the capital of France?"

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

# Voice-to-keyboard typing mode (requires --features typing)
silly typing                      # Type speech into active application
silly typing --input-method direct  # Use direct typing instead of clipboard
```

### Building with Listen feature

```bash
cargo build --release --features listen
```

**Note**: The listen feature uses ScreenCaptureKit for system audio capture, which requires Swift runtime libraries. The `.cargo/config.toml` includes the necessary linker flags.

### Building with Typing feature

```bash
cargo build --release --features typing
```

The typing feature allows you to dictate text directly into any application. Speech is transcribed and typed into the currently focused window.

## Usage

Say the wake word ("Hey Silly" by default) to activate, then speak your question. The CLI will:
1. Show preview text in gray while you're speaking
2. Print final transcription with `>` prefix when you pause
3. Send to LLM and stream the response in cyan
4. Speak the response using TTS (streaming sentence-by-sentence)

After responding, the assistant listens for follow-up questions for 30 seconds (configurable via `wake_timeout_secs`) before requiring the wake word again.

### Keyboard Commands

| Command | Aliases | Description |
|---------|---------|-------------|
| `/mute` | `/mic` | Toggle microphone mute |
| `/speak` | `/tts` | Toggle TTS output |
| `/wake` | | Toggle wake word requirement |
| `/crosstalk` | | Toggle crosstalk mode (listen during TTS) |
| `/aec` | `/echo` | Toggle acoustic echo cancellation |
| `/mode <mode>` | | Switch mode: `chat`, `transcribe`, `note` |
| `/stats` | | Show inference performance stats |
| `/help` | `/h`, `/?` | Show available commands |

Type text and press Enter to submit directly (bypasses transcription).

### Voice Commands

These commands are recognized from speech and processed before the LLM:

| Command | Phrases | Action |
|---------|---------|--------|
| Stop | "stop", "quiet", "shut up", "enough" | Stop TTS playback |
| Mute | "mute", "be quiet" | Disable TTS output |
| Unmute | "unmute", "speak" | Enable TTS output |
| Start Chat | "start chat", "let's chat" | Enter chat mode |
| Start Transcription | "start transcription" | Enter transcribe mode |
| Take Note | "take a note" | Enter note-taking mode |
| Typing Mode | "typing mode", "start typing" | Enter voice-to-keyboard mode |
| Stand Down | "stand down" | Graceful shutdown |

### Application Modes

| Mode | Description |
|------|-------------|
| **Idle** | Default mode. Requires wake word to activate. |
| **Chat** | Conversational mode. No wake word needed, continuous conversation. |
| **Transcribe** | Speech-to-text only. No LLM processing, just transcription. |
| **Note** | Note-taking mode. Transcriptions are appended to a notes file with timestamps. |
| **Typing** | Voice-to-keyboard. Speech is typed into the active application. (requires `--features typing`) |

### Typing Mode Commands

When in typing mode, special voice commands control punctuation, navigation, and editing:

| Category | Commands | Action |
|----------|----------|--------|
| **Punctuation** | "period", "comma", "question mark", "exclamation point" | Insert punctuation |
| **Whitespace** | "enter", "new line", "tab" | Send key press |
| **Editing** | "undo", "redo", "delete", "backspace", "delete word" | Edit operations |
| **Navigation** | "go to end of line", "go to start of line", "select all" | Cursor movement |
| **Control** | "stop typing", "stop", "pause", "resume" | Control typing mode |

**Smart command detection**: Commands are distinguished from text based on:
- Pause duration before speaking (longer pauses suggest commands)
- Phrase length (short phrases like "enter" are likely commands)
- Pattern matching (recognized command phrases)

**Inline punctuation**: Say "hello comma world" and it will type "hello, world"

The current mode is displayed in the status bar with color coding.

### Auto-Submit

Voice input auto-submits after 2 seconds of silence, showing a progress bar. Any typing or new speech cancels the timer.

Press `Ctrl+C` to stop.

The assistant greets you on startup when in full mode.

## Configuration

Create `config.toml` to customize (see [`config.example.toml`](config.example.toml)):

```toml
name = "Silly"
wake_word = "Hey Silly"
wake_timeout_secs = 30  # Seconds to wait for follow-up before requiring wake word again

[llm]
backend = "llama-cpp"
hf_repo = "TheBloke/TinyLlama-1.1B-Chat-v1.0-GGUF"
hf_file = "tinyllama-1.1b-chat-v1.0.Q4_K_M.gguf"
prompt_format = "chatml"  # chatml, mistral, or llama3

# Or use a local model:
# model_path = "models/my-model.gguf"

# Or use OpenAI-compatible API (LM Studio, OpenAI, etc.):
# [llm]
# backend = "openai-compat"
# preset = "lm_studio"  # or "openai" or "ollama"
# model = "model-name"
# api_key = "${OPENAI_API_KEY}"  # optional, for OpenAI

[tts]
engine = "supertonic"
onnx_dir = "models/supertonic/onnx"
voice_style = "models/supertonic/voice_styles/M1.json"
speed = 1.1

[interaction]
# Enable processing input while TTS is playing
crosstalk = false

# Enable acoustic echo cancellation (requires --features aec)
aec = false

# Volume level when user speaks during TTS (0.0-1.0)
duck_volume = 0.2

# Phrases that stop TTS but don't go to LLM
stop_phrases = ["stop", "quiet", "shut up", "enough"]
```

### Configuration Reference

| Setting | Default | Description |
|---------|---------|-------------|
| `name` | "Silly" | Assistant name |
| `wake_word` | "Hey Silly" | Phrase to activate the assistant |
| `wake_timeout_secs` | 30 | After responding, how long to wait for follow-up questions before requiring the wake word again |
| `interaction.crosstalk` | false | When true, continue listening while TTS plays (enables barge-in) |
| `interaction.aec` | false | When true, apply acoustic echo cancellation to remove TTS from mic input |
| `interaction.duck_volume` | 0.2 | TTS volume (0.0-1.0) when user speaks during playback |
| `interaction.stop_phrases` | ["stop", ...] | Phrases that stop TTS without triggering LLM |

### LLM Backends

Silly supports multiple LLM backends:

#### OpenAI-Compatible API (Recommended)

Works with LM Studio, OpenAI, Together.ai, Groq, and any OpenAI-compatible endpoint.

```toml
[llm]
backend = "openai-compat"
preset = "lm_studio"  # or "openai" or "ollama"
model = "model-name"
```

**Presets:**
- `lm_studio` - Local LM Studio server (port 1234)
- `openai` - OpenAI API (requires `api_key`)
- `ollama` - Ollama API mode (port 11434)

**Custom endpoints:**
```toml
[llm]
backend = "openai-compat"
base_url = "https://api.together.xyz/v1"
model = "mistralai/Mixtral-8x7B-Instruct-v0.1"
api_key = "${TOGETHER_API_KEY}"  # Supports environment variables
```

**Supported providers:**
- LM Studio (local)
- OpenAI (gpt-4, gpt-4o, etc.)
- Together.ai
- Groq
- LocalAI
- Text Generation WebUI
- Any OpenAI-compatible endpoint

#### Ollama

Uses the native Ollama Rust SDK for specialized features.

```toml
[llm]
backend = "ollama"
model = "mistral:7b-instruct"
```

#### llama.cpp

Local inference with GGUF models (auto-downloads from HuggingFace).

```toml
[llm]
backend = "llama-cpp"
hf_repo = "TheBloke/Mistral-7B-Instruct-v0.2-GGUF"
hf_file = "mistral-7b-instruct-v0.2.Q4_K_M.gguf"
prompt_format = "mistral"
```

#### Kalosm

Pure Rust inference library.

```toml
[llm]
backend = "kalosm"
model = "phi3"
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

## Runtime State

The application maintains a centralized, thread-safe runtime state that can be queried and modified at runtime. Key state variables:

| State | Description |
|-------|-------------|
| `mic_muted` | Whether microphone input is muted |
| `mic_level` | Current microphone RMS level (0.0-1.0) |
| `tts_enabled` | Whether TTS output is enabled |
| `tts_playing` | Whether TTS is currently playing |
| `tts_volume` | Current TTS volume (0.0-1.0) |
| `tts_level` | Current TTS output RMS level (0.0-1.0) for real-time visualization |
| `crosstalk_enabled` | Whether to process audio during TTS |
| `aec_enabled` | Whether acoustic echo cancellation is active |
| `wake_enabled` | Whether wake word is required |
| `in_conversation` | Whether within wake timeout window |
| `mode` | Current application mode (Idle/Chat/Transcribe/Note) |

All state can be toggled via keyboard commands (e.g., `/mute`, `/crosstalk`, `/aec`).

### Audio Visualization

The status bar displays real-time audio levels using animated Unicode bars:
- **Microphone input**: Green bars (`▁▂▄▆█`) show your voice volume while speaking
- **TTS output**: Magenta bars show the assistant's voice volume during speech playback

Both visualizations use RMS (Root Mean Square) calculation updated every 50ms for smooth animation.

## Profiling with hotpath

```bash
cargo install hotpath --features="tui"
cargo run --release --features hotpath
# In another terminal:
hotpath console
```
