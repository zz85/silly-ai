# Silly CLI Behaviors

## Voice Input

- Audio is continuously captured and processed through VAD (Voice Activity Detection)
- Preview transcription shows gray text while speaking
- Final transcription appends text to the REPL input buffer
- Wake word ("Hey Silly") activates the assistant when not in conversation
- After a response, assistant stays in conversation mode for 30 seconds (configurable)

## Auto-Submit Timer

- On final transcription, a 1.5 second timer starts
- If timer expires without activity, REPL buffer is automatically submitted to LLM
- Activity that cancels the timer:
  - New preview transcription
  - New final transcription
  - Any keyboard input (typing, backspace, delete)
  - Manual Enter key submission

## LLM Streaming

- User input is sent to Ollama for processing
- Response streams token-by-token, displayed in cyan
- Sentences are detected by punctuation (. ! ?)
- Context word count tracked in status bar

## TTS (Text-to-Speech)

- TTS starts as soon as the first complete sentence is available (streaming)
- Sentences are queued to audio sink during LLM streaming
- VAD is muted during TTS playback to prevent feedback

## Cancellation

- Submitting new input cancels any in-progress LLM streaming or TTS playback
- Cancelled requests remove the pending user message from chat history
- UI immediately transitions to processing new input

## Status Bar

- Shows current state with emoji and spinner:
  - 🎤 Listening (green bars, driven by audio level)
  - ⏳ Sending (yellow dots)
  - 💭 Thinking (yellow dots)
  - 🔊 Speaking (magenta music notes ♪♫♪♬)
  - 🔇 Muted (no spinner)
  - ✓ Ready (no spinner)
- Displays context words (📝) and last response words (💬)

## Microphone Mute

- Toggle with Ctrl+M or `/mute` command
- When muted, VAD processing is skipped (no transcription)
- Status shows "🔇 Muted"
- Unmute with same toggle to resume listening

## REPL Input

- Standard line editing: cursor movement, backspace, delete, home, end
- Unicode/CJK character support with proper display width
- Ctrl+C to quit
- Enter to submit (cancels auto-submit timer)
- Voice transcription appends to existing input with space separator
