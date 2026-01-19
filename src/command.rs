//! Command system - intercepts voice/text input before LLM processing
//!
//! Commands are processed in order of priority:
//! 1. Stop commands - halt TTS immediately, don't pass to LLM
//! 2. Mode commands - change application mode
//! 3. Toggle commands - mute/unmute, enable/disable features
//! 4. Pass-through - send to LLM for processing

use crate::config::Config;
use crate::fuzzy::{clean_for_matching, fuzzy_match};
use crate::state::{AppMode, SharedState};

/// Result of command processing
#[derive(Debug, Clone)]
pub enum CommandResult {
    /// Command was handled, do not pass to LLM
    /// Optional string is a response to speak
    Handled(Option<String>),

    /// Mode change requested
    ModeChange {
        mode: AppMode,
        announcement: Option<String>,
    },

    /// Not a command, pass through to LLM
    PassThrough(String),

    /// Stop TTS immediately, no response
    Stop,

    /// Request application shutdown
    Shutdown,
}

/// Command processor - checks input against registered commands
pub struct CommandProcessor {
    /// Stop phrases (exact match, case-insensitive)
    stop_phrases: Vec<String>,

    /// Built-in commands enabled
    builtin_enabled: bool,

    /// Custom commands from config
    custom_commands: Vec<CustomCommandDef>,
}

struct CustomCommandDef {
    phrase: String,
    action: CommandAction,
}

#[derive(Debug, Clone)]
enum CommandAction {
    Mode(AppMode),
    Toggle(ToggleTarget),
    Custom(String),
}

#[derive(Debug, Clone, Copy)]
enum ToggleTarget {
    Mute,
    Tts,
    Crosstalk,
    Wake,
}

impl CommandProcessor {
    /// Create new command processor from config
    pub fn new(config: &Config) -> Self {
        let stop_phrases = config
            .commands
            .stop_phrases
            .iter()
            .map(|s| s.to_lowercase())
            .collect();

        let custom_commands = config
            .commands
            .custom
            .iter()
            .filter_map(|c| {
                let action = parse_action(&c.action)?;
                Some(CustomCommandDef {
                    phrase: c.phrase.to_lowercase(),
                    action,
                })
            })
            .collect();

        Self {
            stop_phrases,
            builtin_enabled: config.commands.enable_builtin,
            custom_commands,
        }
    }

    /// Process input text, returns command result
    pub fn process(&self, text: &str, state: &SharedState) -> CommandResult {
        let text_lower = text.to_lowercase().trim().to_string();
        
        // Trim punctuation for better matching
        let text_trimmed = text_lower.trim_end_matches(|c: char| c.is_ascii_punctuation());

        // 1. Check stop phrases first (highest priority)
        if self.is_stop_command(text_trimmed) {
            return CommandResult::Stop;
        }

        // 2. Check built-in commands
        if self.builtin_enabled {
            if let Some(result) = self.check_builtin(text_trimmed, state) {
                return result;
            }
        }

        // 3. Check custom commands
        if let Some(result) = self.check_custom(text_trimmed, state) {
            return result;
        }

        // 4. Pass through to LLM
        CommandResult::PassThrough(text.to_string())
    }

    /// Check if text is a stop command (with fuzzy matching)
    fn is_stop_command(&self, text: &str) -> bool {
        let text_clean = clean_for_matching(text);
        self.stop_phrases.iter().any(|phrase| {
            let phrase_clean = clean_for_matching(phrase);
            // Exact match or fuzzy match
            text_clean == phrase_clean || fuzzy_match(&phrase_clean, &text_clean)
        })
    }

    /// Check built-in commands
    fn check_builtin(&self, text: &str, state: &SharedState) -> Option<CommandResult> {
        // Shutdown commands
        if text.contains("stand down") || text.contains("standdown") || text == "quit" || text == "exit" {
            return Some(CommandResult::Shutdown);
        }

        // Mode commands
        if text.contains("start chat") || text.contains("let's chat") || text.contains("lets chat") || text.contains("resume") {
            return Some(CommandResult::ModeChange {
                mode: AppMode::Chat,
                announcement: Some("Resuming conversation.".to_string()),
            });
        }

        if text.contains("pause") || text.contains("pause conversation") {
            return Some(CommandResult::ModeChange {
                mode: AppMode::Paused,
                announcement: Some("Conversation paused. Say wake word to resume.".to_string()),
            });
        }

        if text.contains("start transcription") || text.contains("transcribe mode") {
            return Some(CommandResult::ModeChange {
                mode: AppMode::Transcribe,
                announcement: Some("Entering transcription mode.".to_string()),
            });
        }

        if text.contains("take a note") || text.contains("note mode") {
            return Some(CommandResult::ModeChange {
                mode: AppMode::NoteTaking,
                announcement: Some("Entering note-taking mode.".to_string()),
            });
        }


        if text.contains("command mode") || text.contains("commands only") {
            return Some(CommandResult::ModeChange {
                mode: AppMode::Command,
                announcement: Some("Entering command mode. Only commands will be processed.".to_string()),
            });
        }

        // Toggle commands
        if text == "mute" || text == "mute mic" || text == "mute microphone" {
            state.mic_muted.store(true, std::sync::atomic::Ordering::SeqCst);
            return Some(CommandResult::Handled(Some("Microphone muted.".to_string())));
        }

        if text == "unmute" || text == "unmute mic" || text == "unmute microphone" {
            state.mic_muted.store(false, std::sync::atomic::Ordering::SeqCst);
            return Some(CommandResult::Handled(Some("Microphone unmuted.".to_string())));
        }

        if text == "be quiet" || text == "silence" || text == "disable speech" {
            state.tts_enabled.store(false, std::sync::atomic::Ordering::SeqCst);
            return Some(CommandResult::Handled(None)); // No spoken response since TTS is disabled
        }

        if text == "speak" || text == "enable speech" || text == "talk to me" {
            state.tts_enabled.store(true, std::sync::atomic::Ordering::SeqCst);
            return Some(CommandResult::Handled(Some("Speech enabled.".to_string())));
        }

        if text == "enable crosstalk" || text == "crosstalk on" {
            state.crosstalk_enabled.store(true, std::sync::atomic::Ordering::SeqCst);
            return Some(CommandResult::Handled(Some("Crosstalk enabled. I'll keep listening while speaking.".to_string())));
        }

        if text == "disable crosstalk" || text == "crosstalk off" {
            state.crosstalk_enabled.store(false, std::sync::atomic::Ordering::SeqCst);
            return Some(CommandResult::Handled(Some("Crosstalk disabled.".to_string())));
        }

        if text == "disable wake word" || text == "no wake word" {
            state.wake_enabled.store(false, std::sync::atomic::Ordering::SeqCst);
            return Some(CommandResult::Handled(Some("Wake word disabled. I'm always listening.".to_string())));
        }

        if text == "enable wake word" || text == "require wake word" {
            state.wake_enabled.store(true, std::sync::atomic::Ordering::SeqCst);
            return Some(CommandResult::Handled(Some("Wake word enabled.".to_string())));
        }

        None
    }

    /// Check custom commands from config
    fn check_custom(&self, text: &str, state: &SharedState) -> Option<CommandResult> {
        for cmd in &self.custom_commands {
            if text.contains(&cmd.phrase) {
                return Some(execute_action(&cmd.action, state));
            }
        }
        None
    }
}

/// Parse action string from config into CommandAction
fn parse_action(action: &str) -> Option<CommandAction> {
    if action.starts_with("mode:") {
        let mode_str = action.strip_prefix("mode:")?;
        let mode = match mode_str {
            "chat" => AppMode::Chat,
            "paused" | "pause" => AppMode::Paused,
            "transcribe" => AppMode::Transcribe,
            "note" | "notetaking" => AppMode::NoteTaking,
            "command" => AppMode::Command,
            _ => return None,
        };
        return Some(CommandAction::Mode(mode));
    }

    if action.starts_with("toggle:") {
        let target_str = action.strip_prefix("toggle:")?;
        let target = match target_str {
            "mute" => ToggleTarget::Mute,
            "tts" => ToggleTarget::Tts,
            "crosstalk" => ToggleTarget::Crosstalk,
            "wake" => ToggleTarget::Wake,
            _ => return None,
        };
        return Some(CommandAction::Toggle(target));
    }

    // Custom action (for future extension)
    Some(CommandAction::Custom(action.to_string()))
}

/// Execute a command action
fn execute_action(action: &CommandAction, state: &SharedState) -> CommandResult {
    match action {
        CommandAction::Mode(mode) => CommandResult::ModeChange {
            mode: *mode,
            announcement: Some(format!("Switching to {} mode.", mode)),
        },
        CommandAction::Toggle(target) => {
            let (_new_state, msg) = match target {
                ToggleTarget::Mute => {
                    let new = state.toggle_mic_mute();
                    (new, if new { "Microphone muted." } else { "Microphone unmuted." })
                }
                ToggleTarget::Tts => {
                    let new = state.toggle_tts();
                    (new, if new { "Speech enabled." } else { "Speech disabled." })
                }
                ToggleTarget::Crosstalk => {
                    let new = state.toggle_crosstalk();
                    (new, if new { "Crosstalk enabled." } else { "Crosstalk disabled." })
                }
                ToggleTarget::Wake => {
                    let new = state.toggle_wake();
                    (new, if new { "Wake word enabled." } else { "Wake word disabled." })
                }
            };
            CommandResult::Handled(Some(msg.to_string()))
        }
        CommandAction::Custom(action) => {
            // For now, just pass through custom actions
            // Future: implement custom action handlers
            CommandResult::Handled(Some(format!("Custom action: {}", action)))
        }
    }
}

/// Check if input is a slash command (keyboard input)
pub fn process_slash_command(input: &str, state: &SharedState) -> Option<CommandResult> {
    let input = input.trim();
    
    if !input.starts_with('/') {
        return None;
    }

    let cmd = &input[1..].to_lowercase();

    match cmd.as_str() {
        "mute" | "mic" => {
            let muted = state.toggle_mic_mute();
            Some(CommandResult::Handled(Some(
                if muted { "Mic muted".to_string() } else { "Mic unmuted".to_string() }
            )))
        }
        "tts" | "speak" => {
            let enabled = state.toggle_tts();
            Some(CommandResult::Handled(Some(
                if enabled { "TTS enabled".to_string() } else { "TTS disabled".to_string() }
            )))
        }
        "crosstalk" => {
            let enabled = state.toggle_crosstalk();
            Some(CommandResult::Handled(Some(
                if enabled { "Crosstalk enabled".to_string() } else { "Crosstalk disabled".to_string() }
            )))
        }
        "wake" => {
            let enabled = state.toggle_wake();
            Some(CommandResult::Handled(Some(
                if enabled { "Wake word required".to_string() } else { "Wake word disabled".to_string() }
            )))
        }
        "chat" => {
            Some(CommandResult::ModeChange {
                mode: AppMode::Chat,
                announcement: Some("Chat mode".to_string()),
            })
        }
        "transcribe" => {
            Some(CommandResult::ModeChange {
                mode: AppMode::Transcribe,
                announcement: Some("Transcribe mode".to_string()),
            })
        }
        "note" => {
            Some(CommandResult::ModeChange {
                mode: AppMode::NoteTaking,
                announcement: Some("Note mode".to_string()),
            })
        }
        "pause" => {
            Some(CommandResult::ModeChange {
                mode: AppMode::Paused,
                announcement: Some("Paused".to_string()),
            })
        }
        "command" => {
            Some(CommandResult::ModeChange {
                mode: AppMode::Command,
                announcement: Some("Command mode".to_string()),
            })
        }
        "stop" => {
            Some(CommandResult::Stop)
        }
        "quit" | "exit" => {
            Some(CommandResult::Shutdown)
        }
        "status" => {
            let status = format!(
                "Mode: {}, Mic: {}, TTS: {}, Crosstalk: {}, Wake: {}",
                state.mode(),
                if state.mic_muted.load(std::sync::atomic::Ordering::SeqCst) { "muted" } else { "on" },
                if state.tts_enabled.load(std::sync::atomic::Ordering::SeqCst) { "on" } else { "off" },
                if state.crosstalk_enabled.load(std::sync::atomic::Ordering::SeqCst) { "on" } else { "off" },
                if state.wake_enabled.load(std::sync::atomic::Ordering::SeqCst) { "required" } else { "off" },
            );
            Some(CommandResult::Handled(Some(status)))
        }
        "help" | "commands" => {
            let help = "\
Commands:
  /mute - Toggle microphone
  /tts - Toggle text-to-speech
  /crosstalk - Toggle crosstalk (listen during TTS)
  /wake - Toggle wake word requirement
  /chat - Resume conversation mode
  /pause - Pause conversation (requires wake word to resume)
  /transcribe - Enter transcription mode
  /note - Enter note-taking mode
  /command - Enter command-only mode
  /stop - Stop TTS playback
  /quit - Exit application
  /status - Show current status
  /help or /commands - Show this help

Voice commands:
  'stop', 'quiet', 'hush', 'shush' - Stop TTS
  'pause' - Pause conversation
  'resume' - Resume conversation
  'mute' / 'unmute' - Control microphone
  'enable/disable crosstalk' - Control crosstalk
  'command mode' - Enter command-only mode
  'stand down' - Exit application
  
Wake word: Say wake phrase when paused to resume".to_string();
            Some(CommandResult::Handled(Some(help)))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn test_state() -> SharedState {
        RuntimeState::new(&Config::default())
    }

    #[test]
    fn test_stop_commands() {
        let config = Config::default();
        let processor = CommandProcessor::new(&config);
        let state = test_state();

        assert!(matches!(processor.process("stop", &state), CommandResult::Stop));
        assert!(matches!(processor.process("Stop", &state), CommandResult::Stop));
        assert!(matches!(processor.process("STOP", &state), CommandResult::Stop));
        assert!(matches!(processor.process("quiet", &state), CommandResult::Stop));
    }

    #[test]
    fn test_mode_commands() {
        let config = Config::default();
        let processor = CommandProcessor::new(&config);
        let state = test_state();

        let result = processor.process("start chat", &state);
        assert!(matches!(result, CommandResult::ModeChange { mode: AppMode::Chat, .. }));

        let result = processor.process("let's chat", &state);
        assert!(matches!(result, CommandResult::ModeChange { mode: AppMode::Chat, .. }));
    }

    #[test]
    fn test_passthrough() {
        let config = Config::default();
        let processor = CommandProcessor::new(&config);
        let state = test_state();

        let result = processor.process("What's the weather like?", &state);
        assert!(matches!(result, CommandResult::PassThrough(_)));
    }

    #[test]
    fn test_slash_commands() {
        let state = test_state();

        let result = process_slash_command("/mute", &state);
        assert!(result.is_some());

        let result = process_slash_command("/status", &state);
        assert!(result.is_some());

        let result = process_slash_command("not a command", &state);
        assert!(result.is_none());
    }
}
