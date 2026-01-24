# Plan: GGUF Metadata-Based Auto-Configuration for llama-cpp

## Goal

Enhance the `llama-cpp` backend to automatically detect prompt format and context size from GGUF model metadata, reducing manual configuration and improving compatibility with any model.

## Background

GGUF files contain embedded metadata including:
- `tokenizer.chat_template`: Jinja template for message formatting
- `[arch].context_length`: Training context size
- `tokenizer.ggml.eos_token_id`: End-of-sequence token

The `llama-cpp-2` Rust bindings expose this via:
- `model.chat_template(None)` → `LlamaChatTemplate`
- `model.apply_chat_template(template, messages, add_assistant)` → formatted prompt
- `model.n_ctx_train()` → context length
- `model.is_eog_token(token)` → end-of-generation check

## Current State

Partial changes already made:
1. `config.rs`: Added `PromptFormat::Auto` variant with `detect_from_path()` and `resolve()` methods
2. `llm.rs`: Added hardcoded stop sequences per format type
3. `main.rs` & `summarize.rs`: Updated to call `prompt_format.resolve()`

These changes are not yet tested due to pre-existing syntax error in `graphical_ui.rs`.

## Fallback Chain

```
1. Model has embedded chat_template?
   └─ YES → Use apply_chat_template()
   └─ NO  → Continue to step 2

2. User specified explicit prompt_format in config?
   └─ YES (ChatML/Mistral/Llama3) → Use that format
   └─ NO (Auto) → Continue to step 3

3. Detect from filename
   └─ Contains "llama-3/llama3" → Llama3 format
   └─ Contains "mistral" (not mixtral) → Mistral format
   └─ Contains "qwen/tinyllama/yi/mixtral/openhermes" → ChatML format
   └─ No match → ChatML (final fallback)
```

## Implementation Phases

### Phase 1: Enhance `LlamaCppBackend` to Use Model Metadata

**File: `src/llm.rs`**

1. **Add chat template storage to struct**
   ```rust
   pub struct LlamaCppBackend {
       backend: LlamaBackend,
       model: LlamaModel,
       chat_template: Option<LlamaChatTemplate>, // NEW
       system_prompt: String,
       prompt_format: PromptFormat,
       ctx_size: u32,
   }
   ```

2. **Extract chat template in `from_path()` and `from_hf()`**
   - After loading model, call `model.chat_template(None)`
   - Store result (Ok → Some, Err → None)
   - Log which method will be used

3. **Update `format_prompt()` method**
   ```rust
   fn format_prompt(&self, messages: &[Message]) -> String {
       // Priority 1: Use embedded chat template if available and Auto mode
       if self.prompt_format == PromptFormat::Auto {
           if let Some(ref template) = self.chat_template {
               let llama_messages = self.convert_messages(messages);
               if let Ok(prompt) = self.model.apply_chat_template(template, &llama_messages, true) {
                   return prompt;
               }
           }
       }
       // Priority 2: Fall back to PromptFormat enum
       match self.prompt_format {
           PromptFormat::ChatML | PromptFormat::Auto => self.format_chatml(messages),
           PromptFormat::Mistral => self.format_mistral(messages),
           PromptFormat::Llama3 => self.format_llama3(messages),
       }
   }
   ```

4. **Add message conversion helper**
   - Need to check `apply_chat_template` signature for expected message format
   - May need to create `llama_cpp_2::chat::ChatMessage` or similar

5. **Keep stop sequence checks as secondary safeguard**
   - Primary: `model.is_eog_token(token)` (already there)
   - Secondary: String-based checks (already added)

### Phase 2: Add Context Size Auto-Detection

**File: `src/llm.rs`**

1. **In `from_path()` / `from_hf()`:**
   ```rust
   let model_ctx = model.n_ctx_train();
   let effective_ctx = match ctx_size {
       Some(cfg_ctx) if cfg_ctx < model_ctx => cfg_ctx,  // User wants smaller
       Some(cfg_ctx) => {
           eprintln!("Warning: requested ctx {} > model's {}, using model's", cfg_ctx, model_ctx);
           model_ctx
       }
       None => model_ctx,  // Use model default
   };
   eprintln!("Context size: {} (model trained on {})", effective_ctx, model_ctx);
   ```

### Phase 3: Update Config Schema

**File: `src/config.rs`**

1. **Change default `prompt_format` to `Auto`**
   ```rust
   #[derive(Debug, Deserialize, Clone, Copy, Default, PartialEq, Eq)]
   #[serde(rename_all = "lowercase")]
   pub enum PromptFormat {
       #[default]
       Auto,    // Try metadata first, then filename detection
       ChatML,
       Mistral,
       Llama3,
   }
   ```

2. **Make `ctx_size` use 0 as sentinel for auto-detect**
   ```rust
   /// Context size (0 = use model's training context)
   #[serde(default)]
   ctx_size: u32,  // 0 means auto-detect
   ```

### Phase 4: Update Call Sites

**File: `src/main.rs`**
- Minimal changes, pass ctx_size as-is (0 = auto)

**File: `src/summarize.rs`**
- Same as main.rs

## Files to Modify

| File | Changes |
|------|---------|
| `src/llm.rs` | Add chat template field, metadata extraction, new format_prompt logic |
| `src/config.rs` | Change prompt_format default to Auto, adjust ctx_size handling |
| `src/main.rs` | Minor adjustments for ctx_size |
| `src/summarize.rs` | Same as main.rs |

## Resolved: Message Type Conversion

The `apply_chat_template` API signature is:
```rust
pub fn apply_chat_template(
    &self,
    tmpl: &LlamaChatTemplate,
    chat: &[LlamaChatMessage],
    add_ass: bool,  // add assistant turn at end
) -> Result<String, ApplyChatTemplateError>
```

`LlamaChatMessage` is created via:
```rust
LlamaChatMessage::new(role: String, content: String) -> Result<Self, NewLlamaChatMessageError>
```

Where `role` is "system", "user", or "assistant".

**Conversion from our `Message` type is straightforward:**
```rust
fn convert_messages(&self, messages: &[Message]) -> Vec<LlamaChatMessage> {
    let mut result = vec![
        LlamaChatMessage::new("system".to_string(), self.system_prompt.clone()).unwrap()
    ];
    for msg in messages {
        let role = match msg.role {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
        };
        if let Ok(chat_msg) = LlamaChatMessage::new(role.to_string(), msg.content.clone()) {
            result.push(chat_msg);
        }
    }
    result
}
```

### 2. Logging Verbosity

**Decision**: Minimal by default
- "Using embedded chat template" vs "Using ChatML fallback (detected from filename)"

### 3. Error Handling for Template Failures

If `apply_chat_template()` fails at runtime:
- **Recommended**: Warn and fall back to filename-based detection
- Log the error so users know something went wrong

```rust
if let Some(ref template) = self.chat_template {
    match self.model.apply_chat_template(template, &messages, true) {
        Ok(prompt) => return prompt,
        Err(e) => eprintln!("Warning: chat template failed ({}), using fallback", e),
    }
}
// Continue to fallback...
```

## Override Behavior Decision

**Config overrides model template**: If user explicitly sets `prompt_format` to ChatML/Mistral/Llama3, use that format regardless of model metadata. Only `Auto` (default) uses model template.

This lets users work around buggy model templates if needed.

## Testing Plan

1. **With modern GGUF (has embedded template)**
   - Should use `apply_chat_template()` automatically
   - Verify output format matches expected

2. **With older GGUF (no template)**
   - Should fall back to filename detection
   - Should log "Using fallback"

3. **With explicit config override**
   - `prompt_format = "chatml"` should override model template

4. **Context size**
   - No config (0) → use model's n_ctx_train
   - Config smaller than model → use config
   - Config larger than model → warn and use model's

## Next Steps

1. ~~Fix `graphical_ui.rs` syntax error~~ (being done separately)
2. Check `apply_chat_template` API signature in llama-cpp-2
3. Implement Phase 1 (chat template usage)
4. Implement Phase 2 (context size auto-detection)
5. Test with various models
6. Update documentation/config examples
