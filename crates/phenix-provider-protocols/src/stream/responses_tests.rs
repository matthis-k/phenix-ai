use super::*;
use serde_json::json;

fn events() -> Vec<Value> {
    let part = json!({"type":"output_text", "text":"hello", "annotations":[]});
    let item = json!({"id":"msg_1", "type":"message", "role":"assistant", "status":"completed", "content":[part]});
    vec![
        json!({"type":"response.created", "response":{"id":"r1", "status":"in_progress"}}),
        json!({"type":"response.output_item.added", "output_index":0, "item":{"id":"msg_1", "type":"message", "role":"assistant", "status":"in_progress", "content":[]}}),
        json!({"type":"response.content_part.added", "output_index":0, "item_id":"msg_1", "content_index":0, "part":{"type":"output_text", "text":"", "annotations":[]}}),
        json!({"type":"response.output_text.delta", "output_index":0, "item_id":"msg_1", "content_index":0, "delta":"hello"}),
        json!({"type":"response.output_text.done", "output_index":0, "item_id":"msg_1", "content_index":0, "text":"hello"}),
        json!({"type":"response.content_part.done", "output_index":0, "item_id":"msg_1", "content_index":0, "part":part}),
        json!({"type":"response.output_item.done", "output_index":0, "item":item}),
        json!({"type":"response.completed", "response":{"id":"r1", "status":"completed", "output":[item], "usage":{"input_tokens":27, "output_tokens":7}}}),
    ]
}

fn decode(events: &[Value]) -> Result<Stream, BasicError> {
    let mut stream = Stream::new(crate::Protocol::OpenaiResponses);
    for event in events {
        for byte in format!(
            "event: {}\ndata: {event}\n\n",
            event["type"].as_str().unwrap()
        )
        .bytes()
        {
            stream.feed(&[byte])?;
        }
    }
    Ok(stream)
}

#[test]
fn responses_snapshots_do_not_duplicate_streamed_text() {
    let stream = decode(&events()).unwrap();
    assert_eq!(stream.text, "hello");
    assert_eq!(stream.usage.unwrap().input_tokens, 27);
    assert!(stream.done);
}

#[test]
fn responses_reject_identity_order_snapshot_usage_and_terminal_corruption() {
    for index in [1, 3, 4, 5, 6, 7] {
        let mut events = events();
        match index {
            1 => events[index]["output_index"] = json!(1),
            3 => events[index]["item_id"] = json!("foreign"),
            4 => events[index]["text"] = json!("not streamed"),
            5 => events[index]["part"]["text"] = json!("not streamed"),
            6 => events[index]["item"]["content"][0]["text"] = json!("not streamed"),
            7 => events[index]["response"]["usage"]["output_tokens"] = json!(-1),
            _ => unreachable!(),
        }
        assert!(decode(&events).is_err(), "corruption at {index}");
    }
    for removed in [0, 1, 2, 4, 5, 6] {
        let mut events = events();
        events.remove(removed);
        assert!(
            decode(&events).is_err(),
            "missing lifecycle event {removed}"
        );
    }
    let mut events = events();
    events.push(events.last().unwrap().clone());
    assert!(decode(&events).is_err());
}

#[test]
fn responses_errors_are_redacted_and_limits_are_not_success() {
    for kind in ["error", "response.failed", "response.incomplete"] {
        let frame = json!({"type":kind,"message":"secret","response":{"incomplete_details":{"reason":"max_output_tokens"}}});
        let mut stream = Stream::new(crate::Protocol::OpenaiResponses);
        let error = stream.responses_frame(&frame.to_string()).unwrap_err();
        assert_eq!(
            error.code,
            if kind == "response.incomplete" {
                ErrorCode::TurnLimit
            } else {
                ErrorCode::ModelFailed
            }
        );
        assert!(!error.message.contains("secret"));
    }
    let known = json!({"type":"response.failed","response":{
        "error":{"code":"subscription_sharing_usage_limit_exceeded",
                 "message":"TOKEN=private"}
    }});
    let mut stream = Stream::new(crate::Protocol::OpenaiResponses);
    let error = stream.responses_frame(&known.to_string()).unwrap_err();
    assert_eq!(error.code, ErrorCode::ModelFailed);
    assert_eq!(error.message, "subscription_sharing_usage_limit_exceeded");

    let forged = json!({"type":"response.failed","response":{
        "error":{"code":"secret TOKEN=private","message":"TOKEN=private"}
    }});
    let error = stream.responses_frame(&forged.to_string()).unwrap_err();
    assert_eq!(error.message, "HTTP model reported an error");

    assert_eq!(
        crate::Protocol::parse("anthropic-messages").unwrap(),
        crate::Protocol::AnthropicMessages
    );
    assert!(crate::Protocol::parse("").is_err());
    assert!(crate::Protocol::parse("unknown").is_err());
}
