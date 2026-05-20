//! Channel-spec parser.
//!
//! Accepts simple URI forms understood by every CLI subcommand:
//!
//! * `file:///abs/path` ? defaults `follow=0`. `?follow=1` to tail.
//! * `tcp://host:port`
//! * `udp://bind_host:port`
//! * `serial://COM3?baud=115200&data=8&parity=none&stop=1&flow=none`
//! * `process:///bin/sh?args=-c%20echo%20hi` (semi-colon separates)
//! * `mock://tag`
//! * `remote://wss%3A%2F%2Fedge%3A9000%2Fws%3Fsid%3D...%26ch%3D0`
//!
//! The output is a [`ChannelSpec`] from `wanlogger-core`.
//!
//! Boxed `Source` factory is provided by [`open`].

use std::collections::HashMap;

use anyhow::{anyhow, bail, Context, Result};
use wanlogger_core::source::{ChannelSpec, Source};

/// Parse a URI-style spec string into a [`ChannelSpec`].
///
/// # Errors
/// Returns an `anyhow::Error` if the URI is malformed or the kind is
/// unsupported by this build.
pub fn parse(spec: &str) -> Result<ChannelSpec> {
    let (scheme, rest) = spec
        .split_once("://")
        .ok_or_else(|| anyhow!("missing scheme; expected `kind://...`"))?;
    let (body, query) = match rest.split_once('?') {
        Some((b, q)) => (b, parse_query(q)?),
        None => (rest, HashMap::new()),
    };
    Ok(match scheme {
        "file" => ChannelSpec::File {
            path: body.trim_start_matches('/').to_string(),
            follow: query
                .get("follow")
                .is_some_and(|v| matches!(v.as_str(), "1" | "true" | "yes")),
        },
        "tcp" => ChannelSpec::Tcp {
            addr: body.to_string(),
        },
        "udp" => ChannelSpec::Udp {
            bind: body.to_string(),
        },
        "serial" => ChannelSpec::Serial {
            port: body.to_string(),
            baud: parse_num(&query, "baud", 115_200)?,
            data_bits: parse_num(&query, "data", 8)?,
            parity: query
                .get("parity")
                .cloned()
                .unwrap_or_else(|| "none".to_string()),
            stop_bits: parse_num(&query, "stop", 1)?,
            flow: query
                .get("flow")
                .cloned()
                .unwrap_or_else(|| "none".to_string()),
        },
        "process" => ChannelSpec::Process {
            argv: parse_argv(body, &query)?,
        },
        "pipe" => ChannelSpec::Pipe {
            path: body.trim_start_matches('/').to_string(),
        },
        "mock" => ChannelSpec::Mock {
            tag: body.to_string(),
        },
        "remote" => ChannelSpec::Remote {
            url: pct_decode(body),
        },
        other => bail!("unsupported channel kind: {other}"),
    })
}

/// Open a [`Source`] for the given spec.
///
/// **Implementation note:** v0.1 supports `file`, `tcp`, `udp`,
/// `serial`, `process`, `mock` out of the box. Serial I/O requires the
/// `serial` Cargo feature; without it the stub source returns `E-1101`
/// from `Source::open` with a clear feature-gating message.
///
/// # Errors
/// Returns an `anyhow::Error` if the kind is not yet implemented in
/// this build.
pub fn open(spec: &ChannelSpec) -> Result<Box<dyn Source>> {
    use wanlogger_core::source::{
        file::FileSource, mock::MockSource, process::ProcessSource, serial::SerialSource,
        tcp::TcpSource, udp::UdpSource,
    };
    Ok(match spec.clone() {
        ChannelSpec::File { path, follow } => Box::new(FileSource::new(path, follow)),
        ChannelSpec::Tcp { addr } => Box::new(TcpSource::new(addr)),
        ChannelSpec::Udp { bind } => Box::new(UdpSource::new(bind)),
        ChannelSpec::Serial {
            port,
            baud,
            data_bits,
            parity,
            stop_bits,
            flow,
        } => Box::new(SerialSource::new(
            port, baud, data_bits, parity, stop_bits, flow,
        )),
        ChannelSpec::Process { argv } => Box::new(ProcessSource::new(argv)),
        ChannelSpec::Mock { tag } => Box::new(MockSource::new(tag)),
        other => bail!("source kind not yet implemented in CLI: {other:?}"),
    })
}

fn parse_query(q: &str) -> Result<HashMap<String, String>> {
    let mut out = HashMap::new();
    for pair in q.split('&').filter(|s| !s.is_empty()) {
        let (k, v) = pair
            .split_once('=')
            .ok_or_else(|| anyhow!("query param missing `=`: {pair}"))?;
        out.insert(k.to_string(), pct_decode(v));
    }
    Ok(out)
}

fn parse_num<T: std::str::FromStr>(q: &HashMap<String, String>, key: &str, dflt: T) -> Result<T>
where
    T::Err: std::fmt::Display,
{
    match q.get(key) {
        Some(v) => v
            .parse::<T>()
            .map_err(|e| anyhow!("query param `{key}`: {e}")),
        None => Ok(dflt),
    }
}

fn parse_argv(body: &str, q: &HashMap<String, String>) -> Result<Vec<String>> {
    let mut argv = Vec::new();
    if !body.is_empty() {
        argv.push(pct_decode(body.trim_start_matches('/')));
    }
    if let Some(rest) = q.get("args") {
        for a in rest.split(';').filter(|s| !s.is_empty()) {
            argv.push(a.to_string());
        }
    }
    if argv.is_empty() {
        bail!("process spec requires a program path");
    }
    Ok(argv)
}

fn pct_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                out.push((h << 4) | l);
                i += 3;
                continue;
            }
        }
        out.push(if bytes[i] == b'+' { b' ' } else { bytes[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Render a [`ChannelSpec`] back to the URI form parsed by [`parse`].
#[allow(dead_code)]
#[must_use]
pub fn render(spec: &ChannelSpec) -> String {
    match spec {
        ChannelSpec::File { path, follow } => {
            let q = if *follow { "?follow=1" } else { "" };
            format!("file:///{}{q}", path.trim_start_matches('/'))
        }
        ChannelSpec::Tcp { addr } => format!("tcp://{addr}"),
        ChannelSpec::Udp { bind } => format!("udp://{bind}"),
        ChannelSpec::Serial {
            port,
            baud,
            data_bits,
            parity,
            stop_bits,
            flow,
        } => format!(
            "serial://{port}?baud={baud}&data={data_bits}&parity={parity}&stop={stop_bits}&flow={flow}"
        ),
        ChannelSpec::Process { argv } => {
            let prog = argv.first().map_or("", String::as_str);
            let rest: Vec<&str> = argv.iter().skip(1).map(String::as_str).collect();
            if rest.is_empty() {
                format!("process:///{prog}")
            } else {
                format!("process:///{prog}?args={}", rest.join(";"))
            }
        }
        ChannelSpec::Pipe { path } => format!("pipe:///{}", path.trim_start_matches('/')),
        ChannelSpec::Mock { tag } => format!("mock://{tag}"),
        ChannelSpec::Remote { url } => format!("remote://{}", pct_encode(url)),
        other => format!("unsupported://{other:?}"),
    }
    .replace(' ', "%20")
}

/// Stable kind tag used for session-dir naming.
#[must_use]
pub fn kind_tag(spec: &ChannelSpec) -> &'static str {
    match spec {
        ChannelSpec::File { .. } => "file",
        ChannelSpec::Tcp { .. } => "tcp",
        ChannelSpec::Udp { .. } => "udp",
        ChannelSpec::Serial { .. } => "serial",
        ChannelSpec::Process { .. } => "process",
        ChannelSpec::Pipe { .. } => "pipe",
        ChannelSpec::Mock { .. } => "mock",
        ChannelSpec::Replay { .. } => "replay",
        ChannelSpec::Syslog { .. } => "syslog",
        ChannelSpec::Mqtt { .. } => "mqtt",
        ChannelSpec::HttpWebhook { .. } => "http",
        ChannelSpec::Telnet { .. } => "telnet",
        ChannelSpec::Ssh { .. } => "ssh",
        ChannelSpec::Remote { .. } => "remote",
        _ => "other",
    }
}

/// Short interface descriptor used for session-dir naming.
#[must_use]
pub fn iface_tag(spec: &ChannelSpec) -> String {
    match spec {
        ChannelSpec::Serial { port, .. } => sanitize(port),
        ChannelSpec::Tcp { addr }
        | ChannelSpec::Telnet { addr }
        | ChannelSpec::Ssh { addr, .. } => sanitize(addr),
        ChannelSpec::Udp { bind }
        | ChannelSpec::Syslog { bind }
        | ChannelSpec::HttpWebhook { bind, .. } => sanitize(bind),
        ChannelSpec::File { path, .. } | ChannelSpec::Pipe { path } => {
            let last = std::path::Path::new(path)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("file");
            sanitize(last)
        }
        ChannelSpec::Process { argv } => {
            let prog = argv.first().map_or("proc", String::as_str);
            let last = std::path::Path::new(prog)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("proc");
            sanitize(last)
        }
        ChannelSpec::Mqtt { topic, .. } => sanitize(topic),
        ChannelSpec::Mock { tag } => sanitize(tag),
        ChannelSpec::Replay { path } => sanitize(path),
        ChannelSpec::Remote { url } => sanitize(url),
        _ => "iface".to_string(),
    }
}

fn pct_encode(s: &str) -> String {
    use std::fmt::Write as _;

    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(b));
        } else {
            write!(&mut out, "%{b:02X}").expect("writing to String cannot fail");
        }
    }
    out
}

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_') {
                c
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

/// Read a spec from `meta.toml` style key (used by [`crate::cmd::profile`]).
///
/// This is a thin wrapper around `toml::from_str` for ergonomic
/// reuse from subcommands.
///
/// # Errors
/// Returns the underlying parse error.
#[allow(dead_code)]
pub fn from_toml(s: &str) -> Result<ChannelSpec> {
    toml::from_str(s).context("parsing channel spec from TOML")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_tcp() {
        let s = parse("tcp://127.0.0.1:5555").unwrap();
        match s {
            ChannelSpec::Tcp { addr } => assert_eq!(addr, "127.0.0.1:5555"),
            other => panic!("wrong: {other:?}"),
        }
    }

    #[test]
    fn parse_serial_defaults() {
        let s = parse("serial://COM3").unwrap();
        match s {
            ChannelSpec::Serial {
                port, baud, parity, ..
            } => {
                assert_eq!(port, "COM3");
                assert_eq!(baud, 115_200);
                assert_eq!(parity, "none");
            }
            other => panic!("wrong: {other:?}"),
        }
    }

    #[test]
    fn open_serial_constructs_source_metadata() {
        let s = parse("serial://COM3?baud=9600&data=7&parity=even&stop=2&flow=hardware").unwrap();
        let source = open(&s).unwrap();
        let meta = source.metadata();

        assert_eq!(meta.kind, "serial");
        assert_eq!(meta.iface, "COM3");
        assert_eq!(meta.tags["baud"], "9600");
        assert_eq!(meta.tags["data_bits"], "7");
        assert_eq!(meta.tags["parity"], "even");
        assert_eq!(meta.tags["stop_bits"], "2");
        assert_eq!(meta.tags["flow"], "hardware");
    }

    #[test]
    fn parse_file_follow() {
        let s = parse("file:///tmp/log?follow=1").unwrap();
        match s {
            ChannelSpec::File { path, follow } => {
                assert_eq!(path, "tmp/log");
                assert!(follow);
            }
            other => panic!("wrong: {other:?}"),
        }
    }

    #[test]
    fn parse_remote_mirror() {
        // REQ: FR-REMOTE-001
        let s = parse(
            "remote://wss%3A%2F%2Fedge.example.test%3A9000%2Fws%3Fsid%3D00000000-0000-4000-8000-000000000001%26ch%3D0",
        )
        .unwrap();
        match s {
            ChannelSpec::Remote { url } => assert_eq!(
                url,
                "wss://edge.example.test:9000/ws?sid=00000000-0000-4000-8000-000000000001&ch=0"
            ),
            other => panic!("wrong: {other:?}"),
        }
    }

    #[test]
    fn parse_unknown_kind() {
        assert!(parse("xyz://abc").is_err());
        assert!(parse("no-scheme").is_err());
    }

    #[test]
    fn iface_tag_is_filesystem_safe() {
        let s = parse("tcp://127.0.0.1:5555").unwrap();
        assert_eq!(iface_tag(&s), "127.0.0.1-5555");
    }
}
