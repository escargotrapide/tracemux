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
//! * `pcap://Ethernet?snaplen=65535&promisc=1&filter=tcp%20port%20502`
//! * `remote://wss%3A%2F%2Fedge%3A9000%2Fws%3Fsid%3D...%26ch%3D0`
//!
//! The output is a [`ChannelSpec`] from `tracemux-core`.
//!
//! Boxed `Source` factory is provided by [`open`].

// REQ: FR-CLI-PCAP

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{anyhow, bail, Context, Result};
use tracemux_core::source::pcap::{
    PcapConfig, PcapPublishMode, PcapSaveMode, DEFAULT_SNAPLEN, DEFAULT_TIMEOUT_MS,
};
use tracemux_core::source::{ChannelSpec, Source};

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
        "pcap" => parse_pcap(body, &query)?,
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
        "pty" => ChannelSpec::Pty {
            argv: parse_argv(body, &query)?,
            cols: parse_num(&query, "cols", 0)?,
            rows: parse_num(&query, "rows", 0)?,
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
    use tracemux_core::source::{
        file::FileSource, mock::MockSource, pcap::PcapConfig, pcap::PcapSource,
        process::ProcessSource, serial::SerialSource, tcp::TcpSource, udp::UdpSource,
    };
    Ok(match spec.clone() {
        ChannelSpec::File { path, follow } => Box::new(FileSource::new(path, follow)),
        ChannelSpec::Tcp { addr } => Box::new(TcpSource::new(addr)),
        ChannelSpec::Udp { bind } => Box::new(UdpSource::new(bind)),
        ChannelSpec::Pcap { .. } => {
            let config = PcapConfig::from_channel_spec(spec)
                .ok_or_else(|| anyhow!("expected pcap channel spec"))?;
            Box::new(PcapSource::new(config))
        }
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

fn parse_optional_num<T: std::str::FromStr>(
    q: &HashMap<String, String>,
    key: &str,
) -> Result<Option<T>>
where
    T::Err: std::fmt::Display,
{
    match q.get(key) {
        Some(v) if !v.is_empty() => v
            .parse::<T>()
            .map(Some)
            .map_err(|e| anyhow!("query param `{key}`: {e}")),
        _ => Ok(None),
    }
}

fn parse_bool(q: &HashMap<String, String>, key: &str, dflt: bool) -> bool {
    q.get(key)
        .map_or(dflt, |v| matches!(v.as_str(), "1" | "true" | "yes" | "on"))
}

fn parse_bool_alias(q: &HashMap<String, String>, keys: &[&str], dflt: bool) -> bool {
    keys.iter()
        .find_map(|key| q.get(*key))
        .map_or(dflt, |v| matches!(v.as_str(), "1" | "true" | "yes" | "on"))
}

fn query_optional_string(q: &HashMap<String, String>, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| q.get(*key))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn parse_pcap(body: &str, query: &HashMap<String, String>) -> Result<ChannelSpec> {
    let interface = pct_decode(body).trim().to_string();
    let mut config = PcapConfig::new(interface);
    config.display_name = query_optional_string(query, &["display_name", "display"]);
    config.promiscuous = parse_bool_alias(query, &["promiscuous", "promisc"], false);
    config.snaplen = parse_num(query, "snaplen", DEFAULT_SNAPLEN)?;
    config.buffer_bytes =
        parse_optional_num(query, "buffer_bytes")?.or(parse_optional_num(query, "buffer")?);
    config.timeout_ms = if query.contains_key("timeout_ms") {
        parse_num(query, "timeout_ms", DEFAULT_TIMEOUT_MS)?
    } else {
        parse_num(query, "timeout", DEFAULT_TIMEOUT_MS)?
    };
    config.immediate = parse_bool(query, "immediate", false);
    config.filter = query_optional_string(query, &["filter"]);
    config.save_mode = query_optional_string(query, &["save_mode", "save"])
        .as_deref()
        .map(str::parse::<PcapSaveMode>)
        .transpose()
        .map_err(anyhow::Error::msg)?
        .unwrap_or_default();
    config.pcapng_path =
        query_optional_string(query, &["pcapng_path", "pcapng"]).map(PathBuf::from);
    config.publish_mode = query_optional_string(query, &["publish_mode", "publish"])
        .as_deref()
        .map(str::parse::<PcapPublishMode>)
        .transpose()
        .map_err(anyhow::Error::msg)?
        .unwrap_or_default();
    config.validate()?;
    Ok(config.into_channel_spec())
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
        ChannelSpec::Pcap { .. } => render_pcap(spec),
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
        ChannelSpec::Pty { argv, cols, rows } => {
            let prog = argv.first().map_or("", String::as_str);
            let rest: Vec<&str> = argv.iter().skip(1).map(String::as_str).collect();
            let mut q: Vec<String> = Vec::new();
            if !rest.is_empty() {
                q.push(format!("args={}", rest.join(";")));
            }
            if *cols != 0 {
                q.push(format!("cols={cols}"));
            }
            if *rows != 0 {
                q.push(format!("rows={rows}"));
            }
            if q.is_empty() {
                format!("pty:///{prog}")
            } else {
                format!("pty:///{prog}?{}", q.join("&"))
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
        ChannelSpec::Pcap { .. } => "pcap",
        ChannelSpec::Serial { .. } => "serial",
        ChannelSpec::Process { .. } => "process",
        ChannelSpec::Pty { .. } => "pty",
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
        ChannelSpec::Pcap { interface, .. } => sanitize(interface),
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
        ChannelSpec::Pty { argv, .. } => {
            let prog = argv.first().map_or("pty", String::as_str);
            let last = std::path::Path::new(prog)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("pty");
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

fn render_pcap(spec: &ChannelSpec) -> String {
    let Some(config) = PcapConfig::from_channel_spec(spec) else {
        return "unsupported://pcap".to_string();
    };
    let mut query = vec![
        format!("snaplen={}", config.snaplen),
        format!("promisc={}", u8::from(config.promiscuous)),
        format!("timeout_ms={}", config.timeout_ms),
        format!("immediate={}", u8::from(config.immediate)),
        format!("save={}", config.save_mode),
        format!("publish={}", config.publish_mode),
    ];
    if let Some(display_name) = config.display_name {
        query.push(format!("display_name={}", pct_encode(&display_name)));
    }
    if let Some(buffer_bytes) = config.buffer_bytes {
        query.push(format!("buffer_bytes={buffer_bytes}"));
    }
    if let Some(filter) = config.filter {
        query.push(format!("filter={}", pct_encode(&filter)));
    }
    if let Some(path) = config.pcapng_path {
        query.push(format!(
            "pcapng_path={}",
            pct_encode(&path.display().to_string())
        ));
    }
    format!(
        "pcap://{}?{}",
        pct_encode(&config.interface),
        query.join("&")
    )
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
    fn parse_pcap_spec_with_defaults_and_options() {
        let s = parse(
            "pcap://Ethernet%200?snaplen=9000&promisc=1&filter=tcp%20port%20502&publish=sampled",
        )
        .unwrap();
        match &s {
            ChannelSpec::Pcap {
                interface,
                promiscuous,
                snaplen,
                filter,
                save_mode,
                publish_mode,
                ..
            } => {
                assert_eq!(interface, "Ethernet 0");
                assert!(*promiscuous);
                assert_eq!(*snaplen, 9_000);
                assert_eq!(filter.as_deref(), Some("tcp port 502"));
                assert_eq!(*save_mode, PcapSaveMode::Session);
                assert_eq!(*publish_mode, PcapPublishMode::Sampled);
            }
            other => panic!("wrong: {other:?}"),
        }
        assert_eq!(kind_tag(&s), "pcap");
        assert_eq!(iface_tag(&s), "Ethernet-0");
    }

    #[test]
    fn parse_pcap_rejects_invalid_numeric_values() {
        assert!(parse("pcap://eth0?snaplen=0").is_err());
        assert!(parse("pcap://eth0?buffer_bytes=0").is_err());
        assert!(parse("pcap://eth0?snaplen=abc").is_err());
    }

    #[test]
    fn open_pcap_constructs_source_metadata() {
        let s = parse("pcap://eth0?filter=udp&publish=stats-only").unwrap();
        let source = open(&s).unwrap();
        let meta = source.metadata();

        assert_eq!(meta.kind, "pcap");
        assert_eq!(meta.iface, "eth0");
        assert_eq!(meta.tags.get("filter").map(String::as_str), Some("udp"));
        assert_eq!(
            meta.tags.get("publish_mode").map(String::as_str),
            Some("stats-only")
        );
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
