//! `wanlogger send` -> write bytes to a running server session.
//!
//! REQ: FR-CLI-003

use anyhow::{anyhow, bail, Context, Result};
use futures::{SinkExt, StreamExt};
use rmpv::Value;
use tokio::io::AsyncReadExt as _;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;
use wanlogger_core::codec::encode_text;
use wanlogger_server::wire::{decode, encode, Envelope, FrameType};

const SUBPROTOCOL: &str = "wanlogger.v1";

/// CLI options for the send command.
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
    /// Text payload.
    pub text: Option<String>,
    /// Encoding label used for text payloads.
    pub encoding: String,
    /// File payload.
    pub file: Option<std::path::PathBuf>,
    /// Hex payload.
    pub hex: Option<String>,
    /// Optional UDP target.
    pub udp_target: Option<String>,
    /// Whether to wait for acknowledgement.
    pub wait_ack: bool,
}

/// Run the `send` subcommand.
///
/// # Errors
/// Returns an error if the payload cannot be read/decoded, the WSS
/// connection fails, or the server returns a write error.
pub async fn run(options: Options) -> Result<()> {
    // REQ: FR-CLI-003
    let sid = Uuid::parse_str(&options.sid).context("--sid must be a UUID")?;
    let body = read_payload(&options).await?;
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
    let seq = 1;
    let mut payload = vec![(Value::String("body".into()), Value::Binary(body))];
    if let Some(target) = options.udp_target {
        payload.push((Value::String("target".into()), Value::String(target.into())));
    }
    let env = Envelope::new(FrameType::Write, seq, Value::Map(payload))
        .with_sid(sid.to_string())
        .with_ch(options.ch);
    socket
        .send(Message::Binary(
            encode(&env).context("encoding write frame")?,
        ))
        .await
        .context("sending write frame")?;

    if options.wait_ack {
        wait_ack(&mut socket, seq).await?;
    }
    socket.close(None).await.ok();
    Ok(())
}

async fn read_payload(options: &Options) -> Result<Vec<u8>> {
    let selected = usize::from(options.text.is_some())
        + usize::from(options.file.is_some())
        + usize::from(options.hex.is_some());
    if selected > 1 {
        bail!("choose at most one of --text, --file, or --hex");
    }
    if let Some(text) = &options.text {
        let (bytes, had_errors) = encode_text(text, &options.encoding);
        if had_errors {
            bail!(
                "--text contains characters not representable in encoding `{}`",
                options.encoding
            );
        }
        return Ok(bytes);
    }
    if let Some(path) = &options.file {
        return tokio::fs::read(path)
            .await
            .with_context(|| format!("reading {}", path.display()));
    }
    if let Some(hex) = &options.hex {
        return decode_hex(hex);
    }
    let mut stdin = tokio::io::stdin();
    let mut buf = Vec::new();
    stdin.read_to_end(&mut buf).await.context("reading stdin")?;
    Ok(buf)
}

async fn wait_ack<S>(socket: &mut S, seq: u64) -> Result<()>
where
    S: StreamExt<Item = std::result::Result<Message, tokio_tungstenite::tungstenite::Error>>
        + Unpin,
{
    while let Some(msg) = socket.next().await {
        let msg = msg.context("receiving websocket frame")?;
        let Message::Binary(bytes) = msg else {
            continue;
        };
        let env = decode(&bytes).context("decoding server frame")?;
        if env.kind != FrameType::Ctl || env.seq != seq {
            continue;
        }
        let event = payload_str(&env.payload, "event").unwrap_or_default();
        match event {
            "write_ack" => return Ok(()),
            "error" => {
                let id = payload_str(&env.payload, "error_id").unwrap_or("E-????");
                let message = payload_str(&env.payload, "message").unwrap_or("write failed");
                bail!("server returned {id}: {message}");
            }
            _ => {}
        }
    }
    Err(anyhow!("websocket closed before write acknowledgement"))
}

fn decode_hex(input: &str) -> Result<Vec<u8>> {
    let clean: String = input.chars().filter(|c| !c.is_whitespace()).collect();
    if clean.len() % 2 != 0 {
        bail!("--hex must contain an even number of hex digits");
    }
    let mut out = Vec::with_capacity(clean.len() / 2);
    for chunk in clean.as_bytes().chunks_exact(2) {
        let hi = hex_val(chunk[0]).ok_or_else(|| anyhow!("invalid hex digit"))?;
        let lo = hex_val(chunk[1]).ok_or_else(|| anyhow!("invalid hex digit"))?;
        out.push((hi << 4) | lo);
    }
    Ok(out)
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
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

    fn options_with_text(text: &str, encoding: &str) -> Options {
        Options {
            url: "ws://127.0.0.1:9000/ws".to_string(),
            token: None,
            sid: Uuid::nil().to_string(),
            ch: 0,
            text: Some(text.to_string()),
            encoding: encoding.to_string(),
            file: None,
            hex: None,
            udp_target: None,
            wait_ack: false,
        }
    }

    #[test]
    fn hex_decodes_with_whitespace() {
        // REQ: FR-CLI-003
        assert_eq!(decode_hex("48 69 0a").unwrap(), b"Hi\n");
    }

    #[test]
    fn hex_rejects_odd_length() {
        assert!(decode_hex("abc").is_err());
    }

    #[tokio::test]
    async fn text_payload_uses_selected_encoding() {
        // REQ: FR-CLI-004
        let body = read_payload(&options_with_text("\u{3042}", "shift_jis"))
            .await
            .unwrap();
        assert_eq!(body, vec![0x82, 0xA0]);
    }
}
