//! Anthropic Messages API (raw HTTP; Anthropic has no official Rust SDK).

use serde_json::{Value, json};

use crate::{ChatRequest, Endpoint, Event, ModelInfo, Role, ToolCall, Usage, url};

const VERSION: &str = "2023-06-01";
/// Models that take `fallbacks: "default"`: if a safety classifier declines a
/// harmless Bible question, the API answers with a suitable model instead.
const FALLBACK_MODELS: [&str; 4] = ["claude-fable-5-1", "claude-opus-5-5", "claude-opus-5", "claude-sonnet-5-5"];
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";

pub fn request(client: &reqwest::Client, req: &ChatRequest) -> reqwest::RequestBuilder {
    let mut system = vec![json!({"type": "text", "text": req.instructions})];
    if !req.context.is_empty() {
        // The Scripture is the big, stable part: cache it so follow-up questions
        // don't pay for it again
        system.push(json!({"type": "text", "text": req.context, "cache_control": {"type": "ephemeral"}}));
    }
    let mut messages: Vec<Value> = req
        .messages
        .iter()
        .map(|m| {
            json!({
                "role": match m.role { Role::User => "user", Role::Assistant => "assistant" },
                "content": m.content,
            })
        })
        .collect();
    // This answer's lookups: the model's message as it came (its thinking signed, so
    // sent back unchanged), then what each call got
    for (i, round) in req.rounds.iter().enumerate() {
        messages.push(round.said.clone());
        let mut results: Vec<Value> = round
            .calls
            .iter()
            .zip(&round.results)
            .map(|(call, result)| json!({"type": "tool_result", "tool_use_id": call.id, "content": result}))
            .collect();
        // The latest lookups end the part to cache, so the next round reads them cheaply
        if i + 1 == req.rounds.len()
            && let Some(last) = results.last_mut()
        {
            last["cache_control"] = json!({"type": "ephemeral"});
        }
        messages.push(json!({"role": "user", "content": results}));
    }
    let mut body = json!({
        "model": req.model,
        "max_tokens": req.max_tokens.unwrap_or(16000),
        "stream": true,
        "system": system,
        "messages": messages,
    });
    if !req.tools.is_empty() {
        let tools: Vec<Value> =
            req.tools.iter().map(|t| json!({"name": t.name, "description": t.description, "input_schema": t.parameters})).collect();
        body["tools"] = json!(tools);
        if req.no_more_tools {
            body["tool_choice"] = json!({"type": "none"});
        }
    }
    if req.thinking {
        body["thinking"] = json!({"type": "adaptive", "display": "summarized"});
    }
    if let Some(effort) = &req.effort {
        body["output_config"] = json!({"effort": effort});
    }
    let mut request = headers(client.post(url(&req.endpoint.base_url, "messages")), &req.endpoint);
    if FALLBACK_MODELS.contains(&req.model.as_str()) {
        body["fallbacks"] = json!("default");
        request = request.header("anthropic-beta", FALLBACK_BETA);
    }
    request.json(&body)
}

pub fn models_request(client: &reqwest::Client, endpoint: &Endpoint) -> reqwest::RequestBuilder {
    headers(client.get(url(&endpoint.base_url, "models?limit=1000")), endpoint)
}

fn headers(request: reqwest::RequestBuilder, endpoint: &Endpoint) -> reqwest::RequestBuilder {
    request
        .header("x-api-key", endpoint.api_key.as_deref().unwrap_or("").trim())
        .header("anthropic-version", VERSION)
}

pub fn parse_models(json: &Value) -> Vec<ModelInfo> {
    json.get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|m| {
            let id = m.get("id").and_then(Value::as_str)?.to_string();
            let supported = |path: &str| m.pointer(path).and_then(Value::as_bool).unwrap_or(false);
            Some(ModelInfo {
                name: m.get("display_name").and_then(Value::as_str).unwrap_or(&id).to_string(),
                context_window: m.get("max_input_tokens").and_then(Value::as_u64),
                max_output: m.get("max_tokens").and_then(Value::as_u64),
                adaptive_thinking: supported("/capabilities/thinking/types/adaptive/supported"),
                effort: supported("/capabilities/effort/supported"),
                id,
            })
        })
        .collect()
}

#[derive(Default)]
pub struct Decoder {
    /// Prompt tokens from `message_start`, including cache reads and writes
    input: Option<u64>,
    cached: Option<u64>,
    /// The reply's content blocks as they come (thinking, text, tool calls), to send
    /// back when it calls tools; and each tool call's input, streamed as JSON pieces
    blocks: Vec<(Value, String)>,
}

impl Decoder {
    fn block(&mut self, event: &Value) -> Option<&mut (Value, String)> {
        let i = event.get("index").and_then(Value::as_u64)? as usize;
        self.blocks.get_mut(i)
    }
}

impl crate::Decoder for Decoder {
    fn decode(&mut self, data: &str, emit: &mut dyn FnMut(Event)) -> Result<bool, String> {
        let Ok(event) = serde_json::from_str::<Value>(data) else {
            return Ok(false);
        };
        match event.get("type").and_then(Value::as_str).unwrap_or("") {
            "message_start" => {
                let usage = &event["message"]["usage"];
                let n = |k: &str| usage.get(k).and_then(Value::as_u64).unwrap_or(0);
                self.input = Some(n("input_tokens") + n("cache_read_input_tokens") + n("cache_creation_input_tokens"));
                self.cached = usage.get("cache_read_input_tokens").and_then(Value::as_u64);
            }
            "content_block_start" => {
                if let Some(i) = event.get("index").and_then(Value::as_u64).map(|i| i as usize).filter(|&i| i < 256) {
                    while self.blocks.len() <= i {
                        self.blocks.push((Value::Null, String::new()));
                    }
                    self.blocks[i] = (event["content_block"].clone(), String::new());
                }
            }
            "content_block_delta" => {
                let delta = event["delta"].clone();
                let piece = |key: &str| delta.get(key).and_then(Value::as_str).unwrap_or("").to_string();
                match delta.get("type").and_then(Value::as_str) {
                    Some("text_delta") => {
                        let text = piece("text");
                        if let Some((b, _)) = self.block(&event) {
                            b["text"] = json!(format!("{}{}", b["text"].as_str().unwrap_or(""), text));
                        }
                        emit(Event::Text { text });
                    }
                    Some("thinking_delta") => {
                        let text = piece("thinking");
                        if let Some((b, _)) = self.block(&event) {
                            b["thinking"] = json!(format!("{}{}", b["thinking"].as_str().unwrap_or(""), text));
                        }
                        if !text.is_empty() {
                            emit(Event::Reasoning { text });
                        }
                    }
                    Some("signature_delta") => {
                        let signature = piece("signature");
                        if let Some((b, _)) = self.block(&event) {
                            b["signature"] = json!(format!("{}{}", b["signature"].as_str().unwrap_or(""), signature));
                        }
                    }
                    Some("input_json_delta") => {
                        let json = piece("partial_json");
                        if let Some((_, input)) = self.block(&event) {
                            input.push_str(&json);
                        }
                    }
                    _ => {}
                }
            }
            "content_block_stop" => {
                if let Some((b, input)) = self.block(&event)
                    && b.get("type").and_then(Value::as_str) == Some("tool_use")
                {
                    b["input"] = serde_json::from_str(if input.trim().is_empty() { "{}" } else { input }).unwrap_or(json!({}));
                }
            }
            "message_delta" => {
                emit(Event::Usage {
                    usage: Usage {
                        input_tokens: self.input,
                        output_tokens: event.pointer("/usage/output_tokens").and_then(Value::as_u64),
                        cached_tokens: self.cached,
                        round: None,
                    },
                });
                if let Some(reason) = event.pointer("/delta/stop_reason").and_then(Value::as_str) {
                    let reason = match reason {
                        "end_turn" | "stop_sequence" => "stop",
                        "max_tokens" | "model_context_window_exceeded" => "length",
                        other => other,
                    };
                    emit(Event::Done { reason: Some(reason.to_string()) });
                    return Ok(true);
                }
            }
            "error" => {
                let message = event.pointer("/error/message").and_then(Value::as_str).unwrap_or("unknown error");
                return Err(match event.pointer("/error/type").and_then(Value::as_str) {
                    Some("overloaded_error") => "Anthropic is overloaded right now. Try again shortly.".to_string(),
                    _ => format!("The model stopped with an error: {}", message),
                });
            }
            _ => {}
        }
        Ok(false)
    }

    fn take_round(&mut self) -> Option<(Value, String, Vec<ToolCall>)> {
        let blocks: Vec<Value> = std::mem::take(&mut self.blocks)
            .into_iter()
            .map(|(b, _)| b)
            // (An empty text block isn't allowed back)
            .filter(|b| b.is_object() && !(b["type"] == "text" && b["text"].as_str().is_none_or(str::is_empty)))
            .collect();
        let calls: Vec<ToolCall> = blocks
            .iter()
            .filter(|b| b["type"] == "tool_use")
            .map(|b| ToolCall {
                id: b["id"].as_str().unwrap_or("").to_string(),
                name: b["name"].as_str().unwrap_or("").to_string(),
                arguments: b["input"].clone(),
            })
            .collect();
        if calls.is_empty() {
            return None;
        }
        Some((json!({"role": "assistant", "content": blocks}), String::new(), calls))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Decoder as _, Round, Tool};

    #[test]
    fn streams_thinking_text_cached_usage_and_refusal() {
        let mut d = Decoder::default();
        let mut events = Vec::new();
        let mut done = false;
        for line in [
            r#"{"type":"message_start","message":{"usage":{"input_tokens":20,"cache_read_input_tokens":5000,"cache_creation_input_tokens":0}}}"#,
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"Checking."}}"#,
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"x"}}"#,
            r#"{"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"Amen."}}"#,
            r#"{"type":"message_delta","delta":{"stop_reason":"refusal"},"usage":{"output_tokens":7}}"#,
        ] {
            done |= d.decode(line, &mut |e| events.push(e)).unwrap();
        }
        assert!(done);
        assert_eq!(
            events,
            vec![
                Event::Reasoning { text: "Checking.".into() },
                Event::Text { text: "Amen.".into() },
                Event::Usage {
                    usage: Usage { input_tokens: Some(5020), output_tokens: Some(7), cached_tokens: Some(5000), round: None }
                },
                Event::Done { reason: Some("refusal".into()) },
            ]
        );
        // No tool calls: no round
        assert!(d.take_round().is_none());
    }

    #[test]
    fn a_tool_call_is_sent_back_with_its_signed_thinking() {
        let mut d = Decoder::default();
        for line in [
            r#"{"type":"message_start","message":{"usage":{"input_tokens":20}}}"#,
            r#"{"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","signature":""}}"#,
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"Need the WEB."}}"#,
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"sig"}}"#,
            r#"{"type":"content_block_stop","index":0}"#,
            r#"{"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}"#,
            r#"{"type":"content_block_stop","index":1}"#,
            r#"{"type":"content_block_start","index":2,"content_block":{"type":"tool_use","id":"toolu_1","name":"read","input":{}}}"#,
            r#"{"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"{\"references\":"}}"#,
            r#"{"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":" \"John 3:16\", \"translations\": [\"web\"]}"}}"#,
            r#"{"type":"content_block_stop","index":2}"#,
            r#"{"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":30}}"#,
        ] {
            d.decode(line, &mut |_| {}).unwrap();
        }
        let (said, _, calls) = d.take_round().unwrap();
        assert_eq!(calls, vec![ToolCall { id: "toolu_1".into(), name: "read".into(), arguments: json!({"references": "John 3:16", "translations": ["web"]}) }]);
        // The thinking, signed, and the call; not the empty text
        assert_eq!(
            said,
            json!({"role": "assistant", "content": [
                {"type": "thinking", "thinking": "Need the WEB.", "signature": "sig"},
                {"type": "tool_use", "id": "toolu_1", "name": "read", "input": {"references": "John 3:16", "translations": ["web"]}},
            ]})
        );

        let req = ChatRequest {
            endpoint: Endpoint { kind: crate::Kind::Anthropic, base_url: "https://api.anthropic.com/v1".into(), api_key: None },
            model: "m".into(),
            instructions: "i".into(),
            context: String::new(),
            messages: vec![crate::Message { role: Role::User, content: "How does the WEB word John 3:16?".into() }],
            max_tokens: None,
            effort: None,
            thinking: true,
            enable_thinking: None,
            tools: vec![Tool { name: "read", description: "Read".into(), parameters: json!({"type": "object"}) }],
            plain_instructions: None,
            context_window: None,
            rounds: vec![Round { said: said.clone(), reasoning: String::new(), calls, results: vec!["For God so loved".into()] }],
            no_more_tools: true,
        };
        let built = request(&reqwest::Client::new(), &req).build().unwrap();
        let b: Value = serde_json::from_slice(built.body().unwrap().as_bytes().unwrap()).unwrap();
        assert_eq!(b["messages"][1], said);
        assert_eq!(
            b["messages"][2],
            json!({"role": "user", "content": [{"type": "tool_result", "tool_use_id": "toolu_1", "content": "For God so loved", "cache_control": {"type": "ephemeral"}}]})
        );
        assert_eq!(b["tools"][0]["input_schema"], json!({"type": "object"}));
        assert_eq!(b["tool_choice"], json!({"type": "none"}));
    }

    #[test]
    fn stream_errors_are_reported() {
        let mut d = Decoder::default();
        let err = d
            .decode(r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#, &mut |_| {})
            .unwrap_err();
        assert!(err.contains("overloaded"));
    }

    #[test]
    fn model_list_carries_context_and_capabilities() {
        let list = parse_models(&json!({"data": [{
            "id": "claude-opus-5-5", "display_name": "Claude Opus 5.5",
            "max_input_tokens": 1000000, "max_tokens": 128000,
            "capabilities": {"thinking": {"supported": true, "types": {"adaptive": {"supported": true}}},
                             "effort": {"supported": true}}
        }]}));
        let m = &list[0];
        assert_eq!((m.context_window, m.max_output, m.adaptive_thinking, m.effort), (Some(1000000), Some(128000), true, true));
    }
}
