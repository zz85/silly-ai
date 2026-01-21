# Manual Testing Checklist

## Auto-Submit Timer (Critical - Regression-Prone)

Test these scenarios before any release that touches timer logic:

### ✓ Scenario 1: Basic timer
1. Say "hello"
2. Wait 1.5 seconds
3. **Expected**: Input auto-submits to LLM

### ✓ Scenario 2: Multiple utterances (REGRESSION TEST)
1. Say "hello"
2. Wait 1 second (timer at 0.5s remaining)
3. Say "world" 
4. Wait 1.5 seconds
5. **Expected**: Input auto-submits (timer restarted, not continued)
6. **Bug symptom**: If timer doesn't restart, it submits after only 0.5s

### ✓ Scenario 3: Speaking cancels timer
1. Say "hello"
2. Wait 1 second
3. Start saying "world" (preview text appears)
4. **Expected**: Timer progress bar disappears immediately
5. **Bug symptom**: Timer keeps counting down while speaking

### ✓ Scenario 4: Keyboard cancels timer
1. Say "hello"
2. Wait 1 second
3. Press any key
4. **Expected**: Timer cancels, no auto-submit
5. Wait 2 seconds - nothing should happen

## Voice Input

- [ ] Wake word activates assistant
- [ ] Preview text shows while speaking (gray)
- [ ] Final text appends to input buffer
- [ ] Multiple utterances append with spaces

## TTS

- [ ] TTS starts on first complete sentence
- [ ] Speaking during TTS cancels it (barge-in)
- [ ] No feedback loop (TTS doesn't trigger VAD)

## Commands

- [ ] `/mute` toggles microphone
- [ ] `/tts` toggles text-to-speech
- [ ] `/wake` toggles wake word
- [ ] `/stop` cancels LLM generation
- [ ] `/quit` exits cleanly
