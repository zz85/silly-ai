# Silly CLI Behaviors

## UI Modes

Silly has two UI modes:

### Text Mode (Default)
- Traditional REPL interface with text input line
- Status bar with emoji indicators
- Preview text shown in gray while speaking
- Input buffer shown with cursor

### Orb Mode
- Animated orb visualization showing assistant state
- Status bar with text labels (cleaner look)
- Four visualization styles: Blob (default), Ring, Orbs, Sphere
- Input buffer shown at bottom

### Switching Between Modes
- `/ui` - Toggle between text and orb modes
- `/ui text` or `/ui t` - Switch to text mode
- `/ui orb` or `/ui o` - Switch to orb mode
- Command line flags: `--text` or `--orb`

### Orb Mode Controls
- **Tab** - Cycle forward through visualization styles (Blob → Ring → Orbs → Sphere → Blob)
- **Shift+Tab** - Cycle backward through visualization styles
- **`** (backtick) - Cycle through shade patterns
- **d** - Toggle status bar display style (emoji ↔ text)

## Voice Input

- Audio is continuously captured and processed through VAD (Voice Activity Detection)
- Preview transcription shows gray text while speaking
- Final transcription appends text to the REPL input buffer
- Wake word ("Hey Silly") activates the assistant when not in conversation
- After a response, assistant stays in conversation mode for 30 seconds (configurable)

## Auto-Submit Timer

- On final transcription, a 1.5 second timer starts (or restarts if already running)
- If timer expires without activity, REPL buffer is automatically submitted to LLM
- Activity that **cancels** the timer:
  - New preview transcription (user is speaking)
  - Any keyboard input (typing, backspace, delete)
  - Manual Enter key submission
- Activity that **restarts** the timer (fresh 1.5s countdown):
  - New final transcription (user stopped speaking)

**Important**: Preview events always cancel the timer (user is still speaking). Final events always restart the timer with a fresh deadline (user finished speaking, start countdown again).

## LLM Streaming

- User input is sent to Ollama for processing
- Response streams token-by-token, displayed in cyan
- Sentences are detected by punctuation (. ! ?)
- Context word count tracked in status bar

## TTS (Text-to-Speech)

- TTS starts as soon as the first complete sentence is available (streaming)
- Sentences are queued to audio sink during LLM streaming
- VAD is muted during TTS playback to prevent feedback
- Toggle with `/tts` command or Ctrl+T

## Cancellation

- Submitting new input cancels any in-progress LLM streaming or TTS playback
- Cancelled requests remove the pending user message from chat history
- UI immediately transitions to processing new input
- `/stop` command cancels current LLM generation

## Status Bar

### Text Mode (Emoji Style)
- Shows current state with emoji and spinner:
  - 🎤 Listening (green bars, driven by audio level)
  - ⏳ Sending (yellow dots)
  - 💭 Thinking (yellow dots)
  - 🔊 Speaking (magenta music notes ♪♫♪♬)
  - 🔇 Muted (no spinner)
  - ✓ Ready (no spinner)
- Displays context words (📝) and last response words (💬)

### Orb Mode (Text Style)
- Shows current state with text labels:
  - "Listening", "Sending", "Thinking", "Speaking", "Muted", "Ready"
- Cleaner appearance to complement the orb visualization

## Microphone Mute

- Toggle with Ctrl+M or `/mute` command
- When muted, VAD processing is skipped (no transcription)
- Status shows "🔇 Muted" (text mode) or "Muted" (orb mode)
- Unmute with same toggle to resume listening

## REPL Input

- Standard line editing: cursor movement, backspace, delete, home, end
- Unicode/CJK character support with proper display width
- Ctrl+C to quit
- Enter to submit (cancels auto-submit timer)
- Voice transcription appends to existing input with space separator

## Application Modes

- **Chat** (default) - Normal conversation with LLM
- **Transcribe** - Voice-to-text only, no LLM processing
- **Note** - Append transcriptions to notes file
- **Command** - Voice commands only, no LLM
- **Paused** - No processing, mic muted

Switch modes with:
- `/chat` - Chat mode
- `/transcribe` - Transcribe mode
- `/note` - Note-taking mode
- `/command` - Command mode
- `/pause` - Pause mode

## Commands

- `/help` - Show available commands
- `/ui [text|orb]` - Switch UI mode (no args to toggle)
- `/mute` - Toggle microphone
- `/tts` - Toggle text-to-speech
- `/wake` - Toggle wake word requirement
- `/stop` - Stop current LLM generation
- `/quit` or `/exit` - Exit application
- `/stats` - Show performance statistics
- `/status` - Show current settings
