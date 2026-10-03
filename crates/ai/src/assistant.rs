//! The app's Bible-study assistant: what the page sends (a provider, a model, the
//! context the reader chose, the conversation) becomes a provider request with the
//! context and instructions attached, and, if the reader lets it, tools to look up
//! more of the library ([`kjv_core::lookups`]). Shared by the app and the browser
//! preview server.

use serde::Deserialize;

use kjv_core::bundle::DataBundle;
use kjv_core::context::{self, Spec};
use kjv_core::lookups;
use kjv_library::Library;

use crate::{ChatRequest, Endpoint, Kind, Message, Tool, ToolCall, ToolOutput};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AskArgs {
    /// Which saved provider (its API key is looked up by this id, never sent by the page)
    pub provider_id: String,
    pub kind: Kind,
    pub base_url: String,
    pub model: String,
    /// What to attach: passages, translations, commentaries, cross-references
    #[serde(default)]
    pub context: Spec,
    pub messages: Vec<Message>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    #[serde(default)]
    pub effort: Option<String>,
    #[serde(default)]
    pub thinking: bool,
    #[serde(default)]
    pub enable_thinking: Option<bool>,
    /// Let the model look up passages, notes, searches, and lexicon entries itself
    #[serde(default)]
    pub lookups: bool,
    /// The model's context window, when known (what is looked up must fit)
    #[serde(default)]
    pub context_window: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsArgs {
    pub provider_id: String,
    pub kind: Kind,
    pub base_url: String,
}

impl ModelsArgs {
    pub fn endpoint(&self, api_key: Option<String>) -> Endpoint {
        Endpoint { kind: self.kind, base_url: self.base_url.clone(), api_key }
    }
}

/// Build the provider request for `args`, attaching the context it asks for.
pub fn prepare(data: &DataBundle, lib: &Library, args: &AskArgs, api_key: Option<String>) -> Result<ChatRequest, String> {
    let built = context::build(data, lib, &args.context, None)?;
    let tools: Vec<Tool> = if args.lookups {
        lookups::tools(lib).into_iter().map(|t| Tool { name: t.name, description: t.description, parameters: t.parameters }).collect()
    } else {
        Vec::new()
    };
    Ok(ChatRequest {
        endpoint: Endpoint { kind: args.kind, base_url: args.base_url.clone(), api_key },
        model: args.model.clone(),
        instructions: context::instructions_with(lib, &built, args.lookups),
        plain_instructions: args.lookups.then(|| context::instructions(lib, &built)),
        context: built.text,
        messages: args.messages.clone(),
        max_tokens: args.max_tokens,
        effort: args.effort.clone(),
        thinking: args.thinking,
        enable_thinking: args.enable_thinking,
        tools,
        context_window: args.context_window,
        rounds: Vec::new(),
        no_more_tools: false,
    })
}

/// Carry out a tool call the model made, giving at most `room` tokens. (It reads the
/// library: run it where blocking is fine.)
pub fn look_up(data: &DataBundle, lib: &Library, call: &ToolCall, room: usize) -> ToolOutput {
    let found = lookups::run(data, lib, &call.name, &call.arguments, room);
    ToolOutput { label: found.label, text: found.text, failed: found.failed }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_carry_the_context() {
        let a: AskArgs = serde_json::from_value(serde_json::json!({
            "providerId": "p", "kind": "anthropic", "baseUrl": "https://x", "model": "m",
            "context": {
                "passages": [{ "bible": "web", "refs": "ROM.14", "commentaries": ["mhc"] }],
                "translations": ["web", "kjv"], "crossrefs": ["openbible"], "crossrefLimit": 5,
                "crossrefText": true, "original": true
            },
            "messages": [{ "role": "user", "content": "Why?" }],
            "lookups": true, "contextWindow": 1048576
        }))
        .unwrap();
        assert_eq!(a.context.passages[0].refs, "ROM.14");
        assert_eq!(a.context.passages[0].commentaries.as_deref(), Some(&["mhc".to_string()][..]));
        assert_eq!(a.context.passages[0].translations, None);
        assert_eq!((a.context.crossref_limit, a.context.crossref_text, a.context.original), (5, true, true));
        assert_eq!((a.lookups, a.context_window), (true, Some(1048576)));
        // No context: nothing attached, and no looking up unless asked
        let none: AskArgs = serde_json::from_value(serde_json::json!({
            "providerId": "p", "kind": "openai", "baseUrl": "x", "model": "m", "messages": []
        }))
        .unwrap();
        assert!(none.context.passages.is_empty());
        assert!(!none.lookups);
    }
}
