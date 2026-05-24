//! `wanlogger watch` -> subscribe to a running server session and emit JSONL.
//!
//! REQ: FR-CLI-009

use anyhow::{bail, Context, Result};
use futures::{SinkExt, StreamExt};
use rmpv::Value;
use serde_json::{json, Map};
use tokio::io::AsyncWriteExt as _;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use uuid::Uuid;
use wanlogger_core::codec::decode as decode_text;
use wanlogger_server::wire::{decode, encode, Envelope, FrameType};

const SUBPROTOCOL: &str = "wanlogger.v1";
const WATCH_SCHEMA: &str = "wanlogger/watch-frame/v1";

/// CLI options for the watch command.
#[derive(Debug)]
pub struct Options {
    /// WebSocket endpoint.
    pub url: String,
    /// Optional bearer token.
    pub token: Option<String>,
    /// Target session id.
    pub sid: String,
    /// Target channel.
    pub ch: u32,
    /// Encoding for binary body text projection. `auto` discovers it from the source list.
    pub encoding: String,
    /// Stop after this many data frames. `None` watches until the socket closes.
    pub max_frames: Option<u64>,
}

/// Run the `watch` subcommand.
///
/// # Errors
/// Returns an error if the session id is invalid, the WSS connection fails,
/// received frames cannot be decoded, or stdout cannot be written.
pub async fn run(options: Options) -> Result<()> {
    // REQ: FR-CLI-009
    let sid = Uuid::parse_str(&options.sid).context("--sid must be a UUID")?;
    let mut req = options
        .url
        .as_str()
        .into_client_request()
        .context("building websocket request")?;
    let protocol = match options.token.as_deref() {
        Some(token) if !token.is_empty() => format!("{SUBPROTOCOL}, bearer.{token}"),
        _ => SUBPROTOCOL.to_string(),
    };
    req.headers_mut().insert(
        "Sec-WebSocket-Protocol",
        HeaderValue::from_str(&protocol).context("invalid websocket subprotocol header")?,
    );

    let (mut socket, _) = tokio_tungstenite::connect_async(req)
        .await
        .context("connecting websocket")?;
    let mut seq = 1_u64;
    let text_encoding = resolve_watch_encoding(&mut socket, sid, &options.encoding, seq).await?;
    if is_auto_encoding(&options.encoding) {
        seq += 1;
    }
    let sub = Envelope::new(FrameType::Sub, seq, Value::Map(vec![]))
        .with_sid(sid.to_string())
        .with_ch(options.ch);
    socket
        .send(Message::Binary(encode(&sub).context("encoding sub frame")?))
        .await
        .context("sending sub frame")?;

    let mut stdout = tokio::io::stdout();
    let mut seen = 0u64;
    while let Some(msg) = socket.next().await {
        let msg = msg.context("receiving websocket frame")?;
        let Message::Binary(bytes) = msg else {
            continue;
        };
        let env = decode(&bytes).context("decoding server frame")?;
        if env.kind == FrameType::Ctl {
            fail_on_ctl_error(&env)?;
        }
        if !is_target_data(&env, sid, options.ch) {
            continue;
        }
        let line = data_json_line(&env, &text_encoding)?;
        stdout.write_all(line.as_bytes()).await?;
        stdout.write_all(b"\n").await?;
        stdout.flush().await?;
        seen += 1;
        if options.max_frames.is_some_and(|max| seen >= max) {
            break;
        }
    }
    socket.close(None).await.ok();
    Ok(())
}

async fn resolve_watch_encoding(
    socket: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
    sid: Uuid,
    requested: &str,
    seq: u64,
) -> Result<String> {
    let requested = requested.trim();
    if !is_auto_encoding(requested) {
        return Ok(if requested.is_empty() {
            "utf-8".to_string()
        } else {
            requested.to_ascii_lowercase()
        });
    }

    let list = Envelope::new(
        FrameType::Ctl,
        seq,
        value_map(vec![("action", value_str("list"))]),
    );
    socket
        .send(Message::Binary(
            encode(&list).context("encoding list ctl frame")?,
        ))
        .await
        .context("sending list ctl frame")?;

    while let Some(msg) = socket.next().await {
        let msg = msg.context("receiving source list frame")?;
        let Message::Binary(bytes) = msg else {
            continue;
        };
        let env = decode(&bytes).context("decoding source list frame")?;
        if env.kind != FrameType::Ctl {
            continue;
        }
        fail_on_ctl_error(&env)?;
        if payload_str(&env.payload, "event") != Some("sources") {
            continue;
        }
        return Ok(source_list_encoding(&env.payload, sid).unwrap_or_else(|| "utf-8".to_string()));
    }

    bail!("server closed before returning source list")
}

fn is_auto_encoding(value: &str) -> bool {
    value.trim().is_empty() || value.trim().eq_ignore_ascii_case("auto")
}

fn fail_on_ctl_error(env: &Envelope) -> Result<()> {
    let Value::Map(_) = &env.payload else {
        return Ok(());
    };
    if payload_str(&env.payload, "event") != Some("error") {
        return Ok(());
    }
    let id = payload_str(&env.payload, "error_id").unwrap_or("E-????");
    let message = payload_str(&env.payload, "message").unwrap_or("watch failed");
    bail!("server returned {id}: {message}")
}

fn is_target_data(env: &Envelope, sid: Uuid, ch: u32) -> bool {
    env.kind == FrameType::Data
        && env.sid.as_deref() == Some(&sid.to_string())
        && env.ch == Some(ch)
}

fn data_json_line(env: &Envelope, encoding: &str) -> Result<String> {
    if env.kind != FrameType::Data {
        bail!("watch can only render data frames");
    }
    let Value::Map(entries) = &env.payload else {
        bail!("data payload must be a map");
    };
    let mut out = Map::new();
    out.insert("schema".to_string(), json!(WATCH_SCHEMA));
    out.insert("seq".to_string(), json!(env.seq));
    if let Some(sid) = &env.sid {
        out.insert("sid".to_string(), json!(sid));
    }
    if let Some(ch) = env.ch {
        out.insert("ch".to_string(), json!(ch));
    }
    for (key, value) in entries {
        let Some(key) = key.as_str() else {
            continue;
        };
        if key == "body" {
            out.insert(key.to_string(), body_to_json(value, encoding));
        } else {
            out.insert(key.to_string(), value_to_json(value));
        }
    }
    Ok(serde_json::to_string(&serde_json::Value::Object(out))?)
}

fn body_to_json(value: &Value, encoding: &str) -> serde_json::Value {
    match value {
        Value::Binary(bytes) => {
            let mut out = Map::new();
            out.insert("type".to_string(), json!("bin"));
            out.insert("len".to_string(), json!(bytes.len()));
            out.insert("hex".to_string(), json!(hex(bytes)));
            let (text, had_errors) = decode_text(bytes, encoding);
            if !had_errors {
                out.insert("text".to_string(), json!(text));
            }
            serde_json::Value::Object(out)
        }
        other => value_to_json(other),
    }
}

fn source_list_encoding(payload: &Value, sid: Uuid) -> Option<String> {
    let Value::Map(entries) = payload else {
        return None;
    };
    let sources = entries.iter().find_map(|(key, value)| {
        if key.as_str() == Some("sources") {
            value.as_array()
        } else {
            None
        }
    })?;
    let sid_text = sid.to_string();
    for source in sources {
        if payload_str(source, "sid") != Some(sid_text.as_str()) {
            continue;
        }
        if let Some(encoding) = payload_str(source, "encoding") {
            return Some(encoding.to_ascii_lowercase());
        }
        if let Some(decoder) = payload_str(source, "decoder") {
            if let Some(encoding) = decoder.strip_prefix("utf8-text:") {
                return Some(encoding.to_ascii_lowercase());
            }
        }
    }
    None
}

fn value_str(s: &str) -> Value {
    Value::String(s.into())
}

fn value_map(entries: Vec<(&str, Value)>) -> Value {
    Value::Map(
        entries
            .into_iter()
            .map(|(key, value)| (value_str(key), value))
            .collect(),
    )
}

fn value_to_json(value: &Value) -> serde_json::Value {
    match value {
        Value::Nil | Value::Ext(_, _) => serde_json::Value::Null,
        Value::Boolean(v) => json!(v),
        Value::Integer(v) => v.as_i64().map_or_else(
            || {
                v.as_u64()
                    .map_or_else(|| json!(v.to_string()), |n| json!(n))
            },
            |n| json!(n),
        ),
        Value::F32(v) => json!(v),
        Value::F64(v) => json!(v),
        Value::String(v) => json!(v.as_str().unwrap_or_default()),
        Value::Binary(v) => json!({ "type": "bin", "len": v.len(), "hex": hex(v) }),
        Value::Array(items) => serde_json::Value::Array(items.iter().map(value_to_json).collect()),
        Value::Map(entries) => {
            let mut out = Map::new();
            for (key, value) in entries {
                let key = key
                    .as_str()
                    .map_or_else(|| value_to_json(key).to_string(), ToString::to_string);
                out.insert(key, value_to_json(value));
            }
            serde_json::Value::Object(out)
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    const LUT: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(char::from(LUT[usize::from(b >> 4)]));
        out.push(char::from(LUT[usize::from(b & 0x0f)]));
    }
    out
}

fn payload_str<'a>(payload: &'a Value, key: &str) -> Option<&'a str> {
    let Value::Map(entries) = payload else {
        return None;
    };
    entries.iter().find_map(|(k, v)| {
        if k.as_str() == Some(key) {
            v.as_str()
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value_str(s: &str) -> Value {
        Value::String(s.into())
    }

    #[test]
    fn renders_binary_data_as_jsonl() {
        // REQ: FR-CLI-009
        let sid = Uuid::nil();
        let env = Envelope::new(
            FrameType::Data,
            42,
            Value::Map(vec![
                (value_str("sid"), value_str(&sid.to_string())),
                (value_str("ch"), Value::from(0_u64)),
                (value_str("kind"), value_str("bytes")),
                (value_str("body"), Value::Binary(b"AT\r\n".to_vec())),
            ]),
        )
        .with_sid(sid.to_string())
        .with_ch(0);

        let line = data_json_line(&env, "utf-8").unwrap();
        let json: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(json["schema"], WATCH_SCHEMA);
        assert_eq!(json["seq"], 42);
        assert_eq!(json["kind"], "bytes");
        assert_eq!(json["body"]["type"], "bin");
        assert_eq!(json["body"]["len"], 4);
        assert_eq!(json["body"]["hex"], "41540d0a");
        assert_eq!(json["body"]["text"], "AT\r\n");
    }

    #[test]
    fn renders_shift_jis_binary_text_with_selected_encoding() {
        // REQ: FR-CLI-009
        let sid = Uuid::nil();
        let env = Envelope::new(
            FrameType::Data,
            43,
            Value::Map(vec![
                (value_str("sid"), value_str(&sid.to_string())),
                (value_str("ch"), Value::from(0_u64)),
                (value_str("kind"), value_str("bytes")),
                (value_str("body"), Value::Binary(vec![0x82, 0xA0])),
            ]),
        )
        .with_sid(sid.to_string())
        .with_ch(0);

        let line = data_json_line(&env, "shift_jis").unwrap();
        let json: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(json["body"]["hex"], "82a0");
        assert_eq!(json["body"]["text"], "あ");
    }

    #[test]
    fn source_list_encoding_prefers_encoding_then_decoder_label() {
        // REQ: FR-CLI-009
        let sid = Uuid::new_v4();
        let with_encoding = value_map(vec![
            ("event", value_str("sources")),
            (
                "sources",
                Value::Array(vec![value_map(vec![
                    ("sid", value_str(&sid.to_string())),
                    ("decoder", value_str("utf8-text:cp932")),
                    ("encoding", value_str("shift_jis")),
                ])]),
            ),
        ]);
        let decoder_only = value_map(vec![
            ("event", value_str("sources")),
            (
                "sources",
                Value::Array(vec![value_map(vec![
                    ("sid", value_str(&sid.to_string())),
                    ("decoder", value_str("utf8-text:cp932")),
                ])]),
            ),
        ]);

        assert_eq!(
            source_list_encoding(&with_encoding, sid).as_deref(),
            Some("shift_jis")
        );
        assert_eq!(
            source_list_encoding(&decoder_only, sid).as_deref(),
            Some("cp932")
        );
    }

    #[test]
    fn filters_target_channel() {
        let sid = Uuid::nil();
        let env = Envelope::new(FrameType::Data, 1, Value::Map(vec![]))
            .with_sid(sid.to_string())
            .with_ch(3);
        assert!(is_target_data(&env, sid, 3));
        assert!(!is_target_data(&env, sid, 4));
    }

    #[test]
    fn ctl_error_becomes_error() {
        let env = Envelope::new(
            FrameType::Ctl,
            7,
            Value::Map(vec![
                (value_str("event"), value_str("error")),
                (value_str("error_id"), value_str("E-2001")),
                (
                    value_str("message"),
                    value_str("subscription sid is unknown"),
                ),
            ]),
        );
        let err = fail_on_ctl_error(&env).unwrap_err().to_string();
        assert!(err.contains("E-2001"));
        assert!(err.contains("subscription sid is unknown"));
    }
}
