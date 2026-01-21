# Auto-Submit Timer Behavior

## State Machine

```
┌─────────────┐
│   No Timer  │ (initial state)
└──────┬──────┘
       │
       │ Final event (user stopped speaking)
       ▼
┌─────────────┐
│   Running   │ (counting down)
└──────┬──────┘
       │
       ├─► Preview event (user speaking again) ──► Cancel ──► No Timer
       ├─► Keyboard input ──────────────────────► Cancel ──► No Timer
       ├─► Final event (new utterance) ─────────► Restart ─► Running
       └─► Timer expires ───────────────────────► Fire ────► No Timer
```

## Event Handlers

### Preview Events (User is Speaking)
- **Action**: Cancel timer
- **Reason**: User is actively speaking, don't submit yet
- **Implementation**: `auto_submit_deadline = None`

### Final Events (User Stopped Speaking)
- **Action**: Start/restart timer with fresh deadline
- **Reason**: User finished an utterance, start countdown
- **Implementation**: `auto_submit_deadline = Some(now + delay)`

### Keyboard Input
- **Action**: Cancel timer
- **Reason**: User is typing, they'll submit manually
- **Implementation**: Check `has_input_activity()` in periodic loop

## Test Scenarios

### Scenario 1: Normal Flow
1. User speaks "hello" → Preview events → timer cancelled
2. User stops → Final event → timer starts (1.5s)
3. User stays quiet → timer fires after 1.5s ✓

### Scenario 2: Multiple Utterances (Regression Test)
1. User speaks "hello" → Preview → timer cancelled
2. User stops → Final → timer starts (T=0)
3. Wait 1 second (T=1)
4. User speaks "world" → Preview → timer cancelled
5. User stops → Final → timer **restarts** (T=0 again, not T=1)
6. Wait 1.5 seconds → timer fires ✓

**Bug**: If timer doesn't restart, it would fire at T=1.5 (only 0.5s after "world")

### Scenario 3: Keyboard Interruption
1. User speaks → Final → timer starts
2. User types on keyboard → timer cancelled
3. Timer doesn't fire ✓

## Common Pitfalls

1. **Preview events not cancelling timer**: Broken condition like `if !matches!(result, None)` when Preview always returns None
2. **Timer not restarting**: Setting timer only once instead of on every Final event
3. **Conflating input types**: Treating voice input and keyboard input the same way

## Implementation Notes

- Preview events MUST unconditionally cancel timer (user is still speaking)
- Final events MUST set fresh deadline, not check if one exists (restart behavior)
- `append_input()` should NOT set `input_activity` flag (that's for keyboard only)
- See `TESTING.md` for manual testing checklist
