//! OpenAI-compatible chat completions: OpenAI, DeepSeek, OpenRouter, Groq, and local
//! servers (vLLM, Ollama, LM Studio, llama.cpp).

use serde_json::{Value, json};

use crate::think::ThinkSplitter;
use crate::{ChatRequest, Endpoint, Event, ModelInfo, Role, ToolCall, Usage, url};

pub fn request(client: &reqwest::Client, req: &ChatRequest, usage_option: bool) -> reqwest::RequestBuilder {
    let mut system = req.instructions.clone();
    if !req.context.is_empty() {
        // Stable text first so servers with prefix caching reuse it across turns
        system.push_str("\n\n");
        system.push_str(&req.context);
    }
    let mut messages = vec![json!({"role": "system", "content": system})];
    messages.extend(req.messages.iter().map(|m| {
        json!({
            "role": match m.role { Role::User => "user", Role::Assistant => "assistant" },
            "content": m.content,
        })
    }));
    // This answer's lookups: what the model asked for, and what it got
    for round in &req.rounds {
        let mut said = round.said.clone();
        // DeepSeek carries its reasoning on through the lookups of one answer
        if is_deepseek(&req.endpoint.base_url) && !round.reasoning.is_empty() {
            said["reasoning_content"] = json!(round.reasoning);
        }
        messages.push(said);
        for (call, result) in round.calls.iter().zip(&round.results) {
            messages.push(json!({"role": "tool", "tool_call_id": call.id, "content": result}));
        }
    }
    let mut body = json!({"model": req.model, "messages": messages, "stream": true});
    if usage_option {
        body["stream_options"] = json!({"include_usage": true});
    }
    if !req.tools.is_empty() {
        let tools: Vec<Value> = req
            .tools
            .iter()
            .map(|t| json!({"type": "function", "function": {"name": t.name, "description": t.description, "parameters": t.parameters}}))
            .collect();
        body["tools"] = json!(tools);
        if req.no_more_tools {
            body["tool_choice"] = json!("none");
        }
    }
    if let Some(max) = req.max_tokens {
        body["max_tokens"] = json!(max);
    }
    if is_deepseek(&req.endpoint.base_url) {
        // DeepSeek V4 thinks by default; `thinking` turns it on or off, and
        // `reasoning_effort` ("low", "high", "max") sets how hard
        if let Some(on) = req.enable_thinking {
            body["thinking"] = json!({"type": if on { "enabled" } else { "disabled" }});
        }
        if let Some(effort) = &req.effort {
            body["reasoning_effort"] = json!(effort);
        }
    } else {
        if let Some(on) = req.enable_thinking {
            body["chat_template_kwargs"] = json!({"enable_thinking": on});
        }
        if let Some(effort) = &req.effort {
            // OpenAI reasoning models; other servers ignore unknown fields or say so
            if req.endpoint.base_url.contains("api.openai.com") {
                body["reasoning_effort"] = json!(effort);
            }
        }
    }
    auth(client.post(url(&req.endpoint.base_url, "chat/completions")), &req.endpoint).json(&body)
}

/// DeepSeek's own API (not another service that hosts its models)
fn is_deepseek(base_url: &str) -> bool {
    base_url.contains("api.deepseek.com")
}

pub fn models_request(client: &reqwest::Client, endpoint: &Endpoint) -> reqwest::RequestBuilder {
    auth(client.get(url(&endpoint.base_url, "models")), endpoint)
}

fn auth(request: reqwest::RequestBuilder, endpoint: &Endpoint) -> reqwest::RequestBuilder {
    match endpoint.api_key.as_deref().map(str::trim).filter(|k| !k.is_empty()) {
        Some(key) => request.bearer_auth(key),
        None => request,
    }
}

/// Context sizes go by different names: `max_model_len` (vLLM), `context_length`
/// (OpenRouter, LM Studio), `context_window` (Groq).
pub fn parse_models(json: &Value) -> Vec<ModelInfo> {
    let list = json.get("data").or_else(|| json.get("models")).and_then(Value::as_array);
    list.into_iter()
        .flatten()
        .filter_map(|m| {
            let id = m.get("id").and_then(Value::as_str)?.to_string();
            let context_window = ["max_model_len", "context_length", "context_window"]
                .iter()
                .find_map(|k| m.get(*k).and_then(Value::as_u64));
            let max_output = m
                .pointer("/top_provider/max_completion_tokens")
                .or_else(|| m.get("max_completion_tokens"))
                .and_then(Value::as_u64);
            let name = m.get("name").and_then(Value::as_str).unwrap_or(&id).to_string();
            Some(ModelInfo { id, name, context_window, max_output, adaptive_thinking: false, effort: false })
        })
        .collect()
}

#[derive(Default)]
pub struct Decoder {
    think: ThinkSplitter,
    reason: Option<String>,
    /// The reply as it came, to send back with its tool calls: its text, its reasoning,
    /// and each call's id, name, and arguments (streamed in pieces, by index)
    text: String,
    reasoning: String,
    calls: Vec<(String, String, String)>,
}

impl Decoder {
    fn emit_text(&mut self, reasoning: bool, text: String, emit: &mut dyn FnMut(Event)) {
        if reasoning {
            emit(Event::Reasoning { text });
        } else {
            self.text.push_str(&text);
            emit(Event::Text { text });
        }
    }
}

impl crate::Decoder for Decoder {
    fn decode(&mut self, data: &str, emit: &mut dyn FnMut(Event)) -> Result<bool, String> {
        if data.trim() == "[DONE]" {
            self.flush(emit);
            emit(Event::Done { reason: self.reason.take().or(Some("stop".into())) });
            return Ok(true);
        }
        let Ok(chunk) = serde_json::from_str::<Value>(data) else {
            return Ok(false);
        };
        if let Some(message) = chunk.pointer("/error/message").and_then(Value::as_str) {
            return Err(format!("The model stopped with an error: {}", message));
        }
        if let Some(choice) = chunk.pointer("/choices/0") {
            let delta = &choice["delta"];
            // `reasoning_content` (DeepSeek, vLLM) or `reasoning` (OpenRouter, newer vLLM)
            for key in ["reasoning_content", "reasoning"] {
                if let Some(text) = delta.get(key).and_then(Value::as_str).filter(|t| !t.is_empty()) {
                    self.reasoning.push_str(text);
                    emit(Event::Reasoning { text: text.to_string() });
                    break;
                }
            }
            if let Some(text) = delta.get("content").and_then(Value::as_str) {
                for (reasoning, text) in self.think.push(text) {
                    self.emit_text(reasoning, text, emit);
                }
            }
            // A tool call comes in pieces: its id and name first, then its arguments
            for piece in delta.get("tool_calls").and_then(Value::as_array).into_iter().flatten() {
                let index = piece.get("index").and_then(Value::as_u64).map_or(self.calls.len(), |i| i as usize);
                if index > 64 {
                    continue;
                }
                while self.calls.len() <= index {
                    self.calls.push(Default::default());
                }
                let call = &mut self.calls[index];
                if let Some(id) = piece.get("id").and_then(Value::as_str).filter(|s| !s.is_empty()) {
                    call.0 = id.to_string();
                }
                if let Some(name) = piece.pointer("/function/name").and_then(Value::as_str) {
                    call.1.push_str(name);
                }
                if let Some(args) = piece.pointer("/function/arguments").and_then(Value::as_str) {
                    call.2.push_str(args);
                }
            }
            if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
                self.reason = Some(match reason {
                    "length" => "length".into(),
                    "content_filter" => "refusal".into(),
                    "stop" | "tool_calls" | "eos" => "stop".into(),
                    other => other.into(),
                });
            }
        }
        if let Some(u) = chunk.get("usage").filter(|u| u.is_object()) {
            emit(Event::Usage {
                usage: Usage {
                    input_tokens: u.get("prompt_tokens").and_then(Value::as_u64),
                    output_tokens: u.get("completion_tokens").and_then(Value::as_u64),
                    cached_tokens: u
                        .pointer("/prompt_tokens_details/cached_tokens")
                        .or_else(|| u.get("prompt_cache_hit_tokens"))
                        .and_then(Value::as_u64),
                    round: None,
                },
            });
        }
        Ok(false)
    }

    fn flush(&mut self, emit: &mut dyn FnMut(Event)) {
        for (reasoning, text) in self.think.finish() {
            self.emit_text(reasoning, text, emit);
        }
    }

    fn take_round(&mut self) -> Option<(Value, String, Vec<ToolCall>)> {
        let calls: Vec<(String, String, String)> = std::mem::take(&mut self.calls).into_iter().filter(|c| !c.1.is_empty()).collect();
        if calls.is_empty() {
            return None;
        }
        // (A server that gives no ids gets ones of our own: each result must name its call)
        let calls: Vec<ToolCall> = calls
            .into_iter()
            .enumerate()
            .map(|(i, (id, name, args))| ToolCall {
                id: if id.is_empty() { format!("call_{}", i) } else { id },
                name,
                arguments: serde_json::from_str(if args.trim().is_empty() { "{}" } else { &args }).unwrap_or(Value::Null),
            })
            .collect();
        let tool_calls: Vec<Value> = calls
            .iter()
            .map(|c| json!({"id": c.id, "type": "function", "function": {"name": c.name, "arguments": c.arguments.to_string()}}))
            .collect();
        let text = std::mem::take(&mut self.text);
        let said = json!({
            "role": "assistant",
            "content": if text.is_empty() { Value::Null } else { json!(text) },
            "tool_calls": tool_calls,
        });
        Some((said, std::mem::take(&mut self.reasoning), calls))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Decoder as _, Round, Tool};

    fn request_for(base_url: &str) -> ChatRequest {
        ChatRequest {
            endpoint: Endpoint { kind: crate::Kind::OpenAi, base_url: base_url.into(), api_key: None },
            model: "m".into(),
            instructions: "i".into(),
            context: String::new(),
            messages: Vec::new(),
            max_tokens: Some(100),
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

    fn body_of(req: &ChatRequest) -> Value {
        let built = request(&reqwest::Client::new(), req, true).build().unwrap();
        serde_json::from_slice(built.body().unwrap().as_bytes().unwrap()).unwrap()
    }

    /// The JSON body `request` sends for `base_url` with thinking and effort set
    fn body(base_url: &str, thinking: Option<bool>, effort: Option<&str>) -> Value {
        let mut req = request_for(base_url);
        req.enable_thinking = thinking;
        req.effort = effort.map(String::from);
        body_of(&req)
    }

    #[test]
    fn deepseek_takes_its_own_thinking_switch_and_effort() {
        let b = body("https://api.deepseek.com/v1", Some(false), Some("low"));
        assert_eq!(b["thinking"], json!({"type": "disabled"}));
        assert_eq!(b["reasoning_effort"], json!("low"));
        assert!(b.get("chat_template_kwargs").is_none());
        let on = body("https://api.deepseek.com/v1", Some(true), None);
        assert_eq!(on["thinking"], json!({"type": "enabled"}));
        assert!(on.get("reasoning_effort").is_none());
        // Left alone, DeepSeek's defaults apply
        let default = body("https://api.deepseek.com/v1", None, None);
        assert!(default.get("thinking").is_none() && default.get("reasoning_effort").is_none());
        // A local server keeps its chat-template switch, and gets no effort
        let local = body("http://192.168.1.20:8000/v1", Some(false), Some("low"));
        assert_eq!(local["chat_template_kwargs"], json!({"enable_thinking": false}));
        assert!(local.get("thinking").is_none() && local.get("reasoning_effort").is_none());
    }

    fn decode(d: &mut Decoder, lines: &[&str]) -> Vec<Event> {
        let mut events = Vec::new();
        for l in lines {
            d.decode(l, &mut |e| events.push(e)).unwrap();
        }
        events
    }

    #[test]
    fn streams_reasoning_text_usage_and_finish() {
        let events = decode(
            &mut Decoder::default(),
            &[
                r#"{"choices":[{"delta":{"role":"assistant","reasoning_content":"Hmm."}}]}"#,
                r#"{"choices":[{"delta":{"content":"Jesus "}}]}"#,
                r#"{"choices":[{"delta":{"content":"wept."},"finish_reason":"stop"}]}"#,
                r#"{"choices":[],"usage":{"prompt_tokens":120,"completion_tokens":9,"prompt_tokens_details":{"cached_tokens":100}}}"#,
                "[DONE]",
            ],
        );
        assert_eq!(
            events,
            vec![
                Event::Reasoning { text: "Hmm.".into() },
                Event::Text { text: "Jesus ".into() },
                Event::Text { text: "wept.".into() },
                Event::Usage {
                    usage: Usage { input_tokens: Some(120), output_tokens: Some(9), cached_tokens: Some(100), round: None }
                },
                Event::Done { reason: Some("stop".into()) },
            ]
        );
    }

    #[test]
    fn tool_calls_are_gathered_from_their_pieces_and_sent_back() {
        let mut d = Decoder::default();
        decode(
            &mut d,
            &[
                r#"{"choices":[{"delta":{"reasoning_content":"The WEB isn't attached."}}]}"#,
                r#"{"choices":[{"delta":{"content":"Let me look."}}]}"#,
                r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_a","type":"function","function":{"name":"read","arguments":""}}]}}]}"#,
                r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"references\": \"John"}}]}}]}"#,
                r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":" 3:16\"}"}}]}}]}"#,
                r#"{"choices":[{"delta":{"tool_calls":[{"index":1,"id":"call_b","function":{"name":"lexicon","arguments":"{\"strongs\":[\"G26\"]}"}}]},"finish_reason":"tool_calls"}]}"#,
                "[DONE]",
            ],
        );
        let (said, reasoning, calls) = d.take_round().unwrap();
        assert_eq!(reasoning, "The WEB isn't attached.");
        assert_eq!(calls.len(), 2);
        assert_eq!((calls[0].id.as_str(), calls[0].name.as_str()), ("call_a", "read"));
        assert_eq!(calls[0].arguments, json!({"references": "John 3:16"}));
        assert_eq!(calls[1].arguments, json!({"strongs": ["G26"]}));
        assert_eq!(said["content"], json!("Let me look."));
        assert_eq!(said["tool_calls"][0]["function"]["name"], json!("read"));
        // A reply without calls has no round
        assert!(d.take_round().is_none());

        // Sent back: the call, its result, the tools, and DeepSeek's reasoning
        let mut req = request_for("https://api.deepseek.com/v1");
        req.tools = vec![Tool { name: "read", description: "Read".into(), parameters: json!({"type": "object"}) }];
        req.rounds = vec![Round { said, reasoning, calls: calls[..1].to_vec(), results: vec!["For God so loved".into()] }];
        let b = body_of(&req);
        let m = b["messages"].as_array().unwrap();
        assert_eq!(m[1]["tool_calls"][0]["id"], json!("call_a"));
        assert_eq!(m[1]["reasoning_content"], json!("The WEB isn't attached."));
        assert_eq!(m[2], json!({"role": "tool", "tool_call_id": "call_a", "content": "For God so loved"}));
        assert_eq!(b["tools"][0]["function"]["name"], json!("read"));
        assert!(b.get("tool_choice").is_none());
        // Elsewhere the reasoning isn't sent back; and when nothing more may be looked up, it says so
        req.endpoint.base_url = "https://api.openai.com/v1".into();
        req.no_more_tools = true;
        let b = body_of(&req);
        assert!(b["messages"][1].get("reasoning_content").is_none());
        assert_eq!(b["tool_choice"], json!("none"));
    }

    #[test]
    fn reads_context_sizes_from_vllm_and_openrouter() {
        let vllm = json!({"data": [{"id": "qwen3.8-flash-next", "max_model_len": 262144}]});
        let m = &parse_models(&vllm)[0];
        assert_eq!((m.id.as_str(), m.context_window), ("qwen3.8-flash-next", Some(262144)));
        let router = json!({"data": [{"id": "x/y", "name": "Y", "context_length": 128000,
            "top_provider": {"max_completion_tokens": 8192}}]});
        let m = &parse_models(&router)[0];
        assert_eq!((m.name.as_str(), m.context_window, m.max_output), ("Y", Some(128000), Some(8192)));
    }
}
