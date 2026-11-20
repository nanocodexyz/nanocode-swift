use nanocode::{
    HarnessFamily, Model, ModelInfo, ModelSelection,
    antigravity::{AntigravityHarness, DEFAULT_MODEL, Profile},
    codex::{CONTEXT_WINDOW_TOKENS, auto_compact_token_limit},
};
use serde_json::{Value, json};

const CLAUDE_AUTO_COMPACT_WINDOW_TOKENS: u64 = 200_000;
const CLAUDE_COMPACT_RESERVE_TOKENS: u64 = 33_000;
const GEMINI_CONTEXT_WINDOW_TOKENS: u64 = 1_048_576;

fn entry(
    id: &str,
    model: &ModelSelection,
    context_window_tokens: u64,
    auto_compact_limit: Option<u64>,
) -> Value {
    let info = ModelInfo::of(model);
    json!({
        "id": id,
        "name": info.name,
        "efforts": info.efforts,
        "effort": info.effort,
        "priority": info.fast,
        "service_tiers": info.service_tiers,
        "reasoning_modes": info.reasoning_modes,
        "context_window_tokens": context_window_tokens,
        "auto_compact_token_limit": auto_compact_limit,
    })
}

pub fn models() -> Value {
    let claude = HarnessFamily::Claude
        .models()
        .map(|model| {
            let window = nanocode::claude::default_context_window_tokens(model.as_str());
            entry(
                model.as_str(),
                &model,
                window,
                Some(
                    window
                        .min(CLAUDE_AUTO_COMPACT_WINDOW_TOKENS)
                        .saturating_sub(CLAUDE_COMPACT_RESERVE_TOKENS),
                ),
            )
        })
        .collect::<Vec<_>>();
    let codex = Model::ALL
        .into_iter()
        .map(|model| {
            let window = CONTEXT_WINDOW_TOKENS.min(model.max_context_window_tokens());
            entry(
                model.as_str(),
                &ModelSelection::Codex(model),
                window,
                auto_compact_token_limit(model.as_str(), window),
            )
        })
        .collect::<Vec<_>>();
    let antigravity = [DEFAULT_MODEL]
        .into_iter()
        .map(|model| {
            entry(
                model,
                &AntigravityHarness::selection(model),
                GEMINI_CONTEXT_WINDOW_TOKENS,
                Some(Profile::for_model(model).compaction_limit),
            )
        })
        .collect::<Vec<_>>();
    json!({"harnesses": [
        {"id": "claude", "models": claude},
        {"id": "codex", "models": codex},
        {"id": "antigravity", "models": antigravity},
    ]})
}
