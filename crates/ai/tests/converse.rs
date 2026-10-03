//! The look-things-up loop ([`kjv_ai::converse`]) against a stand-in OpenAI-compatible
//! server on localhost: rounds, what is sent back, a server that takes no tools, and
//! the limit on rounds.

use std::sync::{Arc, Mutex};

use kjv_ai::{ChatRequest, Endpoint, Event, Kind, Message, Role, Tool, ToolCall, ToolOutput, Usage};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

type Requests = Arc<Mutex<Vec<Value>>>;

/// A server answering each request's JSON body with `respond` (status, SSE body).
async fn serve(respond: impl Fn(&Value) -> (u16, String) + Send + Sync + 'static) -> (String, Requests) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let requests: Requests = Arc::default();
    let seen = requests.clone();
    let respond = Arc::new(respond);
    tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let (seen, respond) = (seen.clone(), respond.clone());
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 8192];
                // Headers, then as much body as they say
                let body_start = loop {
                    let n = socket.read(&mut chunk).await.unwrap();
                    buf.extend_from_slice(&chunk[..n]);
                    if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        break i + 4;
                    }
                };
                let head = String::from_utf8_lossy(&buf[..body_start]).to_lowercase();
                let length: usize = head.lines().find_map(|l| l.strip_prefix("content-length:")).map_or(0, |v| v.trim().parse().unwrap());
                while buf.len() < body_start + length {
                    let n = socket.read(&mut chunk).await.unwrap();
                    buf.extend_from_slice(&chunk[..n]);
                }
                let body: Value = serde_json::from_slice(&buf[body_start..body_start + length]).unwrap();
                let (status, text) = respond(&body);
                seen.lock().unwrap().push(body);
                let kind = if status == 200 { "text/event-stream" } else { "application/json" };
                let reply = format!("HTTP/1.1 {} X\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", status, kind, text.len(), text);
                socket.write_all(reply.as_bytes()).await.unwrap();
                socket.shutdown().await.ok();
            });
        }
    });
    (url, requests)
}

fn sse(events: &[Value]) -> String {
    let mut out: String = events.iter().map(|e| format!("data: {}\n\n", e)).collect();
    out.push_str("data: [DONE]\n\n");
    out
}

fn call_to_read() -> String {
    sse(&[
        json!({"choices": [{"delta": {"content": "Let me check."}}]}),
        json!({"choices": [{"delta": {"tool_calls": [{"index": 0, "id": "call_1", "type": "function", "function": {"name": "read", "arguments": "{\"references\": \"John 3:16\"}"}}]}, "finish_reason": "tool_calls"}]}),
        json!({"choices": [], "usage": {"prompt_tokens": 100, "completion_tokens": 10}}),
    ])
}

fn request(url: &str) -> ChatRequest {
    ChatRequest {
        instructions: "Look things up.".into(),
        plain_instructions: Some("Answer from what is attached.".into()),
        messages: vec![Message { role: Role::User, content: "How does the WEB word John 3:16?".into() }],
        tools: vec![Tool { name: "read", description: "Read".into(), parameters: json!({"type": "object"}) }],
        ..ChatRequest::new(Endpoint { kind: Kind::OpenAi, base_url: url.into(), api_key: None }, "m")
    }
}

async fn converse(req: &ChatRequest, calls: Arc<Mutex<Vec<ToolCall>>>) -> Vec<Event> {
    let mut events = Vec::new();
    kjv_ai::converse(
        &kjv_ai::client(),
        req,
        |call, room| {
            let calls = calls.clone();
            async move {
                assert!(room > 0);
                calls.lock().unwrap().push(call);
                ToolOutput { label: "John 3:16 · WEB".into(), text: "16 For God so loved the world".into(), failed: false }
            }
        },
        |e| events.push(e),
    )
    .await
    .unwrap();
    events
}

#[tokio::test]
async fn a_round_of_looking_up_then_the_answer() {
    let (url, requests) = serve(|body| {
        let looked_up = body["messages"].as_array().unwrap().iter().any(|m| m["role"] == "tool");
        if looked_up {
            (200, sse(&[
                json!({"choices": [{"delta": {"content": "The WEB has it."}, "finish_reason": "stop"}]}),
                json!({"choices": [], "usage": {"prompt_tokens": 150, "completion_tokens": 5}}),
            ]))
        } else {
            (200, call_to_read())
        }
    })
    .await;
    let calls = Arc::default();
    let events = converse(&request(&url), Arc::clone(&calls)).await;
    let usage = |input, output, round| Event::Usage { usage: Usage { input_tokens: Some(input), output_tokens: Some(output), cached_tokens: None, round: Some(round) } };
    assert_eq!(
        events,
        vec![
            Event::Text { text: "Let me check.".into() },
            usage(100, 10, 0),
            Event::Lookup { id: "call_1".into(), tool: "read".into(), label: "John 3:16 · WEB".into(), tokens: 8, text: "16 For God so loved the world".into(), failed: false },
            Event::Text { text: "The WEB has it.".into() },
            usage(150, 5, 1),
            Event::Done { reason: Some("stop".into()) },
        ]
    );
    assert_eq!(calls.lock().unwrap()[0].arguments, json!({"references": "John 3:16"}));
    // The second request: the model's call, then what it got
    let sent = requests.lock().unwrap();
    assert_eq!(sent.len(), 2);
    let m = sent[1]["messages"].as_array().unwrap();
    assert_eq!(m[2]["tool_calls"][0]["id"], json!("call_1"));
    assert_eq!(m[2]["content"], json!("Let me check."));
    assert_eq!(m[3], json!({"role": "tool", "tool_call_id": "call_1", "content": "16 For God so loved the world"}));
    assert!(sent[1].get("tool_choice").is_none());
}

#[tokio::test]
async fn a_server_that_takes_no_tools_is_asked_again_without_them() {
    let (url, requests) = serve(|body| {
        if body.get("tools").is_some() {
            (400, json!({"error": {"message": "registry.ollama.ai/library/x does not support tools"}}).to_string())
        } else {
            (200, sse(&[json!({"choices": [{"delta": {"content": "From what is attached."}, "finish_reason": "stop"}]})]))
        }
    })
    .await;
    let events = converse(&request(&url), Arc::default()).await;
    assert_eq!(events, vec![Event::Text { text: "From what is attached.".into() }, Event::Done { reason: Some("stop".into()) }]);
    let sent = requests.lock().unwrap();
    assert_eq!(sent.len(), 2);
    // Asked again with the instructions for answering without tools
    assert_eq!(sent[1]["messages"][0]["content"], json!("Answer from what is attached."));
}

#[tokio::test]
async fn looking_up_stops_after_a_few_rounds() {
    let (url, requests) = serve(|body| {
        if body.get("tool_choice") == Some(&json!("none")) {
            (200, sse(&[json!({"choices": [{"delta": {"content": "With what I have."}, "finish_reason": "stop"}]})]))
        } else {
            (200, call_to_read())
        }
    })
    .await;
    let calls: Arc<Mutex<Vec<ToolCall>>> = Arc::default();
    let events = converse(&request(&url), Arc::clone(&calls)).await;
    assert_eq!(calls.lock().unwrap().len(), 6, "six rounds");
    assert_eq!(events.last(), Some(&Event::Done { reason: Some("stop".into()) }));
    assert_eq!(events.iter().filter(|e| matches!(e, Event::Done { .. })).count(), 1);
    // Each round's text is its own: the answer comes last
    assert_eq!(events.iter().rev().find_map(|e| if let Event::Text { text } = e { Some(text.as_str()) } else { None }), Some("With what I have."));
    assert_eq!(requests.lock().unwrap().len(), 7);
}
