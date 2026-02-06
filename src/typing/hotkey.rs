//! Global hotkey detection for typing mode
//!
//! Detects hotkeys to control voice typing:
//! - Double-tap Command key: Toggle on/off
//! - Ctrl+Space: Push-to-talk (hold to talk, release to stop)

use rdev::{listen, Event, EventType, Key};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

/// Hotkey events sent to the main thread
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HotkeyEvent {
    /// Toggle voice typing on/off (double-tap Cmd)
    Toggle,
    /// Push-to-talk started (Ctrl+Space pressed)
    PushToTalkStart,
    /// Push-to-talk ended (Ctrl+Space released)
    PushToTalkEnd,
}

/// Configuration for hotkey detection
pub struct HotkeyConfig {
    /// Maximum time between key presses for double-tap (ms)
    pub double_tap_threshold_ms: u64,
    /// Enable double-tap Command hotkey for toggle
    pub enable_double_tap_cmd: bool,
    /// Enable Ctrl+Space for push-to-talk
    pub enable_ctrl_space_ptt: bool,
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            double_tap_threshold_ms: 400, // 400ms between taps
            enable_double_tap_cmd: true,
            enable_ctrl_space_ptt: true,
        }
    }
}

/// Start the global hotkey listener
///
/// Returns a receiver for hotkey events and a handle to stop the listener.
pub fn start_hotkey_listener(
    config: HotkeyConfig,
) -> Result<(mpsc::Receiver<HotkeyEvent>, Arc<AtomicBool>), String> {
    let (tx, rx) = mpsc::channel();
    let running = Arc::new(AtomicBool::new(true));
    let running_clone = Arc::clone(&running);

    thread::spawn(move || {
        let mut last_meta_release: Option<Instant> = None;
        let mut meta_pressed = false;
        let mut ctrl_pressed = false;
        let mut _space_pressed = false;
        let mut ptt_active = false;
        let double_tap_threshold = Duration::from_millis(config.double_tap_threshold_ms);

        // Track if any other key was pressed while Meta was held
        let mut other_key_pressed_with_meta = false;

        let callback = move |event: Event| {
            if !running_clone.load(Ordering::SeqCst) {
                return;
            }

            match event.event_type {
                EventType::KeyPress(key) => {
                    match key {
                        Key::MetaLeft | Key::MetaRight => {
                            meta_pressed = true;
                            other_key_pressed_with_meta = false; // Reset on new press
                        }
                        Key::ControlLeft | Key::ControlRight => {
                            ctrl_pressed = true;
                        }
                        Key::Space => {
                            _space_pressed = true;
                            // Ctrl+Space push-to-talk START
                            if config.enable_ctrl_space_ptt
                                && ctrl_pressed
                                && !meta_pressed
                                && !ptt_active
                            {
                                ptt_active = true;
                                let _ = tx.send(HotkeyEvent::PushToTalkStart);
                            }
                            // Mark other key pressed for double-tap detection
                            if meta_pressed {
                                other_key_pressed_with_meta = true;
                            }
                        }
                        _ => {
                            // Any other key pressed while meta is held
                            if meta_pressed {
                                other_key_pressed_with_meta = true;
                            }
                        }
                    }
                }
                EventType::KeyRelease(key) => {
                    match key {
                        Key::MetaLeft | Key::MetaRight => {
                            // Double-tap Cmd detection for TOGGLE
                            if config.enable_double_tap_cmd
                                && meta_pressed
                                && !other_key_pressed_with_meta
                            {
                                // Clean meta release (no other keys pressed)
                                let now = Instant::now();

                                if let Some(last) = last_meta_release {
                                    if now.duration_since(last) < double_tap_threshold {
                                        // Double-tap detected!
                                        let _ = tx.send(HotkeyEvent::Toggle);
                                        last_meta_release = None; // Reset
                                    } else {
                                        last_meta_release = Some(now);
                                    }
                                } else {
                                    last_meta_release = Some(now);
                                }
                            }
                            meta_pressed = false;
                        }
                        Key::ControlLeft | Key::ControlRight => {
                            ctrl_pressed = false;
                            // If PTT was active and Ctrl is released, end PTT
                            if ptt_active && config.enable_ctrl_space_ptt {
                                ptt_active = false;
                                let _ = tx.send(HotkeyEvent::PushToTalkEnd);
                            }
                        }
                        Key::Space => {
                            _space_pressed = false;
                            // If PTT was active and Space is released, end PTT
                            if ptt_active && config.enable_ctrl_space_ptt {
                                ptt_active = false;
                                let _ = tx.send(HotkeyEvent::PushToTalkEnd);
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        };

        // This blocks until an error occurs
        if let Err(e) = listen(callback) {
            eprintln!("Hotkey listener error: {:?}", e);
        }
    });

    Ok((rx, running))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = HotkeyConfig::default();
        assert_eq!(config.double_tap_threshold_ms, 400);
        assert!(config.enable_double_tap_cmd);
        assert!(config.enable_ctrl_space_ptt);
    }
}
