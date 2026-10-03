//! Streaming chat for the app's Bible-study assistant. Three wire formats cover the
//! services people use: OpenAI-compatible (OpenAI, DeepSeek, OpenRouter, Groq, and
//! local servers such as vLLM, Ollama, LM Studio, llama.cpp), Anthropic, and Gemini.
//!
//! Requests run in Rust, not the web page, so API keys never reach page scripts and
//! local servers on the user's network are reachable from every platform.
//!
//! The model can be given tools ([`converse`]): it asks for something, the app looks
//! it up, and the model carries on with what it found, a few rounds at most, all in
//! one answer.

mod anthropic;
pub mod assistant;
pub mod conversations;
mod gemini;
mod openai;
mod sse;
mod think;

use std::future::Future;
use std::time::Duration;

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use sse::SseParser;
pub use think::ThinkSplitter;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// `POST {base}/chat/completions`
    OpenAi,
    /// `POST {base}/messages`
    Anthropic,
    /// `POST {base}/models/{model}:streamGenerateContent`
    Gemini,
}

/// Where to send requests.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Endpoint {
    pub kind: Kind,
    /// "https://api.openai.com/v1", "http://192.168.1.20:8000/v1", …
    pub base_url: String,
    #[serde(default)]
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatRequest {
    pub endpoint: Endpoint,
    pub model: String,
    /// How the assistant should behave
    pub instructions: String,
    /// Scripture attached to the conversation; cached where the provider allows
    #[serde(default)]
    pub context: String,
    pub messages: Vec<Message>,
    /// Longest answer, in tokens
    #[serde(default)]
    pub max_tokens: Option<u32>,
    /// "low" | "medium" | "high": reasoning effort for models that take one
    #[serde(default)]
    pub effort: Option<String>,
    /// Ask for visible reasoning summaries (models whose `ModelInfo` says they can)
    #[serde(default)]
    pub thinking: bool,
    /// Turn a local reasoning model's thinking on or off (`chat_template_kwargs`,
    /// understood by vLLM and llama.cpp); None leaves the server's default
    #[serde(default)]
    pub enable_thinking: Option<bool>,
    /// Tools the model may call; none, and it answers from what it was given
    #[serde(skip)]
    pub tools: Vec<Tool>,
    /// The instructions to give instead if the server turns out not to take tools
    #[serde(skip)]
    pub plain_instructions: Option<String>,
    /// The model's context window, when known: what is looked up must fit in it
    #[serde(default)]
    pub context_window: Option<u64>,
    /// This answer's rounds of looking things up so far
    #[serde(skip)]
    pub rounds: Vec<Round>,
    /// Nothing more may be looked up: the model answers now
    #[serde(skip)]
    pub no_more_tools: bool,
}

impl ChatRequest {
    /// A request to `model`, with everything else left to be set.
    pub fn new(endpoint: Endpoint, model: impl Into<String>) -> ChatRequest {
        ChatRequest {
            endpoint,
            model: model.into(),
            instructions: String::new(),
            context: String::new(),
            messages: Vec::new(),
            max_tokens: None,
            effort: None,
            thinking: false,
            enable_thinking: None,
            tools: Vec::new(),
            plain_instructions: None,
            context_window: None,
            rounds: Vec::new(),
            no_more_tools: false,
        }
    }
}

/// A tool the model may call.
#[derive(Debug, Clone)]
pub struct Tool {
    pub name: &'static str,
    pub description: String,
    /// Its arguments, as a JSON Schema object (the subset every provider takes: types,
    /// properties, required, enum, items, descriptions)
    pub parameters: Value,
}

/// A tool call the model made.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolCall {
    /// The provider's id for it, which its result must name
    pub id: String,
    pub name: String,
    /// The arguments (null if the model's weren't JSON)
    pub arguments: Value,
}

/// What came of a tool call.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolOutput {
    /// What was looked up, for the reader: "John 3:16 · WEB"
    pub label: String,
    /// What the model is given
    pub text: String,
    /// The call couldn't be carried out (`text` says why, for the model)
    pub failed: bool,
}

/// One round of looking things up in the answer being written.
#[derive(Debug, Clone, Default)]
pub struct Round {
    /// The model's message asking for it, as the provider sent it, to be sent back
    /// exactly (some providers sign what the model thought)
    pub said: Value,
    /// Its reasoning, for providers that want it back within the answer (DeepSeek)
    pub reasoning: String,
    pub calls: Vec<ToolCall>,
    /// What each call got, in order
    pub results: Vec<String>,
}

/// Something the model produced, in order.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Event {
    Text { text: String },
    /// Reasoning the model shows while it thinks (summaries on some services)
    Reasoning { text: String },
    Usage { usage: Usage },
    /// The model looked something up: what (for the reader), how much it got, and the
    /// text it was given
    Lookup { id: String, tool: String, label: String, tokens: usize, text: String, failed: bool },
    /// The answer is complete. `reason` is normalized: "stop", "length", "refusal", or
    /// the provider's own word for anything else.
    Done { reason: Option<String> },
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    /// Input tokens read from the provider's prompt cache
    pub cached_tokens: Option<u64>,
    /// Which request of the answer this is, from 0, when it looked things up (each
    /// round of looking up is a request of its own, with what was found added)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub round: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    /// Context window in tokens, when the service says
    pub context_window: Option<u64>,
    /// Longest possible answer, when the service says
    pub max_output: Option<u64>,
    /// Takes `thinking: adaptive` (Anthropic)
    pub adaptive_thinking: bool,
    /// Takes a reasoning effort level (Anthropic)
    pub effort: bool,
}

/// The HTTP client type, so callers needn't depend on reqwest.
pub type Client = reqwest::Client;

/// One client for the whole app: connection reuse, sane timeouts.
pub fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        // Between chunks, not the whole answer: local models can think for minutes
        .read_timeout(Duration::from_secs(300))
        .user_agent(concat!("Scriptorium/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("HTTP client builds")
}

/// Stream a reply, calling `emit` for each piece. Dropping the returned future
/// closes the connection, which stops generation on the server.
pub async fn chat(client: &reqwest::Client, req: &ChatRequest, mut emit: impl FnMut(Event) + Send) -> Result<(), String> {
    let mut decoder = decoder(req.endpoint.kind);
    let reason = stream(client, req, &mut *decoder, &mut emit).await.map_err(StreamError::message)?;
    emit(Event::Done { reason });
    Ok(())
}

/// Most rounds of looking things up in one answer, and calls in all: then it answers
/// with what it has
const MAX_ROUNDS: usize = 6;
const MAX_CALLS: usize = 16;
/// Most tokens one lookup gives the model
const LOOKUP_TOKENS: usize = 24_000;
/// Tokens all of an answer's lookups may come to when the context window isn't known
const LOOKUPS_UNKNOWN_WINDOW: usize = 100_000;

/// Stream a reply as [`chat`] does, letting the model call `req.tools`: `run(call,
/// room)` carries out a call, giving at most `room` tokens, and the model carries on
/// with what it got. Its text and reasoning stream throughout; each lookup is an
/// [`Event::Lookup`]; [`Event::Done`] comes once, at the end.
pub async fn converse<F, Fut>(client: &reqwest::Client, req: &ChatRequest, mut run: F, mut emit: impl FnMut(Event) + Send) -> Result<(), String>
where
    F: FnMut(ToolCall, usize) -> Fut + Send,
    Fut: Future<Output = ToolOutput> + Send,
{
    let mut req = req.clone();
    let mut calls = 0;
    // What the request holds before anything is looked up, and what lookups have added
    let base = req.messages.iter().fold(estimate(&req.instructions) + estimate(&req.context), |n, m| n + estimate(&m.content));
    let mut looked_up = 0;
    loop {
        let round = req.rounds.len() as u32;
        let tag = (!req.tools.is_empty()).then_some(round);
        let mut decoder = decoder(req.endpoint.kind);
        let mut forward = |event: Event| match event {
            Event::Usage { mut usage } => {
                usage.round = tag;
                emit(Event::Usage { usage });
            }
            other => emit(other),
        };
        let reason = match stream(client, &req, &mut *decoder, &mut forward).await {
            Ok(reason) => reason,
            // A server that doesn't take tools: the question again, without them
            Err(StreamError::NoTools(_)) if round == 0 => {
                req.tools.clear();
                if let Some(plain) = req.plain_instructions.take() {
                    req.instructions = plain;
                }
                decoder = self::decoder(req.endpoint.kind);
                stream(client, &req, &mut *decoder, &mut forward).await.map_err(StreamError::message)?
            }
            Err(e) => return Err(e.message()),
        };
        let asked = decoder.take_round().filter(|(_, _, c)| !c.is_empty() && !req.tools.is_empty() && !req.no_more_tools);
        let Some((said, reasoning, round_calls)) = asked else {
            emit(Event::Done { reason });
            return Ok(());
        };
        let mut results = Vec::with_capacity(round_calls.len());
        for call in &round_calls {
            calls += 1;
            let room = match req.context_window {
                Some(window) => (window as usize).saturating_sub(base + looked_up + req.max_tokens.unwrap_or(16_000) as usize + 2_000),
                None => LOOKUPS_UNKNOWN_WINDOW.saturating_sub(looked_up),
            }
            .min(LOOKUP_TOKENS);
            let out = if room < 500 {
                ToolOutput {
                    label: "Nothing more: no room".into(),
                    text: "There is no room left in this conversation to look up more. Answer with what you have, and say what you couldn't look up.".into(),
                    failed: true,
                }
            } else {
                run(call.clone(), room).await
            };
            let tokens = estimate(&out.text);
            looked_up += tokens;
            emit(Event::Lookup { id: call.id.clone(), tool: call.name.clone(), label: out.label, tokens, text: out.text.clone(), failed: out.failed });
            results.push(out.text);
        }
        req.rounds.push(Round { said, reasoning, calls: round_calls, results });
        if req.rounds.len() >= MAX_ROUNDS || calls >= MAX_CALLS {
            req.no_more_tools = true;
        }
    }
}

fn estimate(text: &str) -> usize {
    kjv_core::context::estimate_tokens(text)
}

fn decoder(kind: Kind) -> Box<dyn Decoder> {
    match kind {
        Kind::OpenAi => Box::new(openai::Decoder::default()),
        Kind::Anthropic => Box::new(anthropic::Decoder::default()),
        Kind::Gemini => Box::new(gemini::Decoder::default()),
    }
}

enum StreamError {
    /// The server doesn't take tools (its message)
    NoTools(String),
    Other(String),
}

impl StreamError {
    fn message(self) -> String {
        match self {
            StreamError::NoTools(m) | StreamError::Other(m) => m,
        }
    }
}

/// One request: its text, reasoning, and usage to `emit` as they come. Returns why it
/// stopped (not sent as an event: the caller decides when the answer is done).
async fn stream(
    client: &reqwest::Client,
    req: &ChatRequest,
    decoder: &mut dyn Decoder,
    emit: &mut (dyn FnMut(Event) + Send),
) -> Result<Option<String>, StreamError> {
    if req.model.trim().is_empty() {
        return Err(StreamError::Other("Choose a model first.".into()));
    }
    let request = match req.endpoint.kind {
        Kind::OpenAi => openai::request(client, req, true),
        Kind::Anthropic => anthropic::request(client, req),
        Kind::Gemini => gemini::request(client, req),
    };
    let mut response = send(request).await.map_err(StreamError::Other)?;
    if response.status() == reqwest::StatusCode::BAD_REQUEST {
        let body = response.text().await.unwrap_or_default();
        // Local servers that take no tools say so ("does not support tools", "tool
        // choice requires --enable-auto-tool-choice", "tools param requires --jinja")
        if !req.tools.is_empty() && req.endpoint.kind == Kind::OpenAi && body.to_lowercase().contains("tool") {
            return Err(StreamError::NoTools(http_error(reqwest::StatusCode::BAD_REQUEST, &body)));
        }
        // Some OpenAI-compatible servers reject the usage option; ask again without it
        if req.endpoint.kind != Kind::OpenAi || !body.contains("stream_options") {
            return Err(StreamError::Other(http_error(reqwest::StatusCode::BAD_REQUEST, &body)));
        }
        response = send(openai::request(client, req, false)).await.map_err(StreamError::Other)?;
    }
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(StreamError::Other(http_error(status, &body)));
    }

    let mut parser = SseParser::default();
    let mut stream = response.bytes_stream();
    let mut reason = None;
    let mut done = |event: Event, reason: &mut Option<Option<String>>| match event {
        Event::Done { reason: r } => *reason = Some(r),
        other => emit(other),
    };
    let mut events = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| StreamError::Other(format!("The connection dropped: {}", plain_error(&e))))?;
        for data in parser.push(&chunk) {
            decoder.decode(&data, &mut |e| events.push(e)).map_err(StreamError::Other)?;
            for e in events.drain(..) {
                done(e, &mut reason);
            }
        }
    }
    for data in parser.finish() {
        decoder.decode(&data, &mut |e| events.push(e)).map_err(StreamError::Other)?;
    }
    decoder.flush(&mut |e| events.push(e));
    for e in events.drain(..) {
        done(e, &mut reason);
    }
    Ok(reason.flatten())
}

/// Models the service offers, with context sizes where it reports them.
pub async fn models(client: &reqwest::Client, endpoint: &Endpoint) -> Result<Vec<ModelInfo>, String> {
    let request = match endpoint.kind {
        Kind::OpenAi => openai::models_request(client, endpoint),
        Kind::Anthropic => anthropic::models_request(client, endpoint),
        Kind::Gemini => gemini::models_request(client, endpoint),
    };
    let response = send(request.timeout(Duration::from_secs(30))).await?;
    let status = response.status();
    let body = response.text().await.map_err(|e| plain_error(&e))?;
    if !status.is_success() {
        return Err(http_error(status, &body));
    }
    let json: serde_json::Value =
        serde_json::from_str(&body).map_err(|_| "The server's model list isn't JSON. Check the address.".to_string())?;
    let mut list = match endpoint.kind {
        Kind::OpenAi => openai::parse_models(&json),
        Kind::Anthropic => anthropic::parse_models(&json),
        Kind::Gemini => gemini::parse_models(&json),
    };
    list.sort_by_key(|m| m.name.to_lowercase());
    Ok(list)
}

/// Turns provider stream payloads into events. Returns true once the reply is done.
trait Decoder: Send {
    fn decode(&mut self, data: &str, emit: &mut dyn FnMut(Event)) -> Result<bool, String>;
    fn flush(&mut self, _emit: &mut dyn FnMut(Event)) {}
    /// After the reply: if the model called tools, its message as the provider wants it
    /// back, its reasoning, and the calls
    fn take_round(&mut self) -> Option<(Value, String, Vec<ToolCall>)> {
        None
    }
}

fn url(base: &str, path: &str) -> String {
    format!("{}/{}", base.trim().trim_end_matches('/'), path.trim_start_matches('/'))
}

async fn send(request: reqwest::RequestBuilder) -> Result<reqwest::Response, String> {
    request.send().await.map_err(|e| {
        if e.is_connect() {
            format!("Couldn't reach the server: {}", plain_error(&e))
        } else if e.is_timeout() {
            "The server took too long to answer.".to_string()
        } else if e.is_builder() {
            "That server address isn't a valid URL.".to_string()
        } else {
            plain_error(&e)
        }
    })
}

/// The innermost cause, which is the useful part ("connection refused").
fn plain_error(e: &(dyn std::error::Error + 'static)) -> String {
    let mut source = e;
    while let Some(next) = source.source() {
        source = next;
    }
    source.to_string()
}

/// A readable message for a failed request, using the service's own message when it
/// sends one (every supported format nests it at `error.message`).
fn http_error(status: reqwest::StatusCode, body: &str) -> String {
    let detail = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|j| {
            j.pointer("/error/message")
                .or_else(|| j.pointer("/message"))
                .or_else(|| j.get("detail"))
                .and_then(|m| m.as_str().map(str::to_string))
        })
        .unwrap_or_else(|| body.chars().take(300).collect::<String>().trim().to_string());
    let lead = match status.as_u16() {
        401 | 403 => "The service rejected the API key".to_string(),
        404 => "Not found. Check the server address and model name".to_string(),
        429 => "Rate limited or out of credit".to_string(),
        413 => "The request is too large for this model".to_string(),
        500..=599 => format!("The service had an error ({})", status.as_u16()),
        _ => format!("Request failed ({})", status.as_u16()),
    };
    if detail.is_empty() { format!("{}.", lead) } else { format!("{}: {}", lead, detail) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_join_cleanly() {
        assert_eq!(url("http://192.168.1.20:8000/v1/", "/chat/completions"), "http://192.168.1.20:8000/v1/chat/completions");
        assert_eq!(url(" https://api.anthropic.com/v1", "messages"), "https://api.anthropic.com/v1/messages");
    }

    #[test]
    fn errors_use_the_services_message() {
        let body = r#"{"error":{"message":"invalid x-api-key","type":"authentication_error"}}"#;
        assert_eq!(
            http_error(reqwest::StatusCode::UNAUTHORIZED, body),
            "The service rejected the API key: invalid x-api-key"
        );
        assert_eq!(http_error(reqwest::StatusCode::BAD_GATEWAY, ""), "The service had an error (502).");
    }
}
