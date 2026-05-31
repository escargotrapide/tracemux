use std::time::Duration;

use anyhow::{anyhow, bail, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Eol {
    None,
    Lf,
    Crlf,
}

#[derive(Debug, Clone)]
pub(crate) struct ScenarioConfig {
    pub(crate) send_text: Vec<String>,
    pub(crate) send_hex: Vec<String>,
    pub(crate) repeat: u32,
    pub(crate) initial_delay_ms: u64,
    pub(crate) interval_ms: u64,
    pub(crate) eol: Eol,
    pub(crate) chunk_size: Option<usize>,
    pub(crate) echo: bool,
    pub(crate) ack_prefix: String,
    pub(crate) read_chunk: usize,
    pub(crate) idle_timeout_ms: Option<u64>,
}

#[derive(Debug, Clone)]
pub(crate) struct Scenario {
    payloads: Vec<Vec<u8>>,
    repeat: u32,
    initial_delay: Duration,
    interval: Duration,
    chunk_size: Option<usize>,
    echo: bool,
    ack_prefix: Vec<u8>,
    read_chunk: usize,
    idle_timeout: Option<Duration>,
}

impl Scenario {
    pub(crate) fn from_config(config: ScenarioConfig) -> Result<Self> {
        if config.read_chunk == 0 {
            bail!("read_chunk must be greater than zero");
        }
        if config.chunk_size == Some(0) {
            bail!("chunk_size must be greater than zero when provided");
        }

        let mut payloads = Vec::with_capacity(config.send_text.len() + config.send_hex.len());
        for text in config.send_text {
            let mut bytes = text.into_bytes();
            config.eol.append_to(&mut bytes);
            payloads.push(bytes);
        }
        for hex in config.send_hex {
            payloads.push(parse_hex(&hex)?);
        }

        Ok(Self {
            payloads,
            repeat: config.repeat,
            initial_delay: Duration::from_millis(config.initial_delay_ms),
            interval: Duration::from_millis(config.interval_ms),
            chunk_size: config.chunk_size,
            echo: config.echo,
            ack_prefix: config.ack_prefix.into_bytes(),
            read_chunk: config.read_chunk,
            idle_timeout: config.idle_timeout_ms.map(Duration::from_millis),
        })
    }

    pub(crate) const fn initial_delay(&self) -> Duration {
        self.initial_delay
    }

    pub(crate) const fn interval(&self) -> Duration {
        self.interval
    }

    pub(crate) const fn read_chunk(&self) -> usize {
        self.read_chunk
    }

    pub(crate) const fn idle_timeout(&self) -> Option<Duration> {
        self.idle_timeout
    }

    pub(crate) fn scripted_payloads(&self) -> Vec<Vec<u8>> {
        let mut out = Vec::with_capacity(self.payloads.len() * self.repeat as usize);
        for _ in 0..self.repeat {
            out.extend(self.payloads.iter().cloned());
        }
        out
    }

    pub(crate) fn chunks<'a>(&self, payload: &'a [u8]) -> Vec<&'a [u8]> {
        match self.chunk_size {
            Some(size) => payload.chunks(size).collect(),
            None => vec![payload],
        }
    }

    pub(crate) fn echo_payload(&self, inbound: &[u8]) -> Option<Vec<u8>> {
        if !self.echo {
            return None;
        }
        let mut out = Vec::with_capacity(self.ack_prefix.len() + inbound.len());
        out.extend_from_slice(&self.ack_prefix);
        out.extend_from_slice(inbound);
        Some(out)
    }
}

impl Eol {
    fn append_to(self, bytes: &mut Vec<u8>) {
        match self {
            Self::None => {}
            Self::Lf => bytes.push(b'\n'),
            Self::Crlf => bytes.extend_from_slice(b"\r\n"),
        }
    }
}

fn parse_hex(input: &str) -> Result<Vec<u8>> {
    let compact: String = input
        .strip_prefix("0x")
        .unwrap_or(input)
        .chars()
        .filter(|c| !c.is_whitespace() && !matches!(c, '_' | '-'))
        .collect();
    if compact.len() % 2 != 0 {
        bail!("hex payload must have an even number of digits");
    }
    let mut out = Vec::with_capacity(compact.len() / 2);
    for idx in (0..compact.len()).step_by(2) {
        let pair = &compact[idx..idx + 2];
        let byte =
            u8::from_str_radix(pair, 16).map_err(|e| anyhow!("invalid hex byte `{pair}`: {e}"))?;
        out.push(byte);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_config() -> ScenarioConfig {
        ScenarioConfig {
            send_text: Vec::new(),
            send_hex: Vec::new(),
            repeat: 1,
            initial_delay_ms: 0,
            interval_ms: 0,
            eol: Eol::None,
            chunk_size: None,
            echo: false,
            ack_prefix: "ACK:".to_string(),
            read_chunk: 16,
            idle_timeout_ms: Some(10),
        }
    }

    #[test]
    fn text_payloads_apply_eol_and_repeat() {
        let mut config = base_config();
        config.send_text = vec!["hello".to_string()];
        config.eol = Eol::Lf;
        config.repeat = 2;
        let scenario = Scenario::from_config(config).unwrap();

        assert_eq!(scenario.scripted_payloads(), vec![b"hello\n", b"hello\n"]);
    }

    #[test]
    fn initial_delay_is_configurable() {
        let mut config = base_config();
        config.initial_delay_ms = 250;
        let scenario = Scenario::from_config(config).unwrap();

        assert_eq!(scenario.initial_delay(), Duration::from_millis(250));
    }

    #[test]
    fn hex_payloads_accept_separators() {
        let mut config = base_config();
        config.send_hex = vec!["48 65_6c-6c6f".to_string()];
        let scenario = Scenario::from_config(config).unwrap();

        assert_eq!(scenario.scripted_payloads(), vec![b"Hello"]);
    }

    #[test]
    fn chunks_split_payload() {
        let mut config = base_config();
        config.chunk_size = Some(2);
        let scenario = Scenario::from_config(config).unwrap();

        assert_eq!(
            scenario.chunks(b"abcde"),
            vec![&b"ab"[..], &b"cd"[..], &b"e"[..]]
        );
    }

    #[test]
    fn echo_payload_uses_prefix() {
        let mut config = base_config();
        config.echo = true;
        config.ack_prefix = "ok:".to_string();
        let scenario = Scenario::from_config(config).unwrap();

        assert_eq!(scenario.echo_payload(b"cmd").unwrap(), b"ok:cmd");
    }

    #[test]
    fn invalid_hex_is_rejected() {
        let mut config = base_config();
        config.send_hex = vec!["abc".to_string()];

        assert!(Scenario::from_config(config).is_err());
    }

    #[test]
    fn zero_sizes_are_rejected() {
        let mut config = base_config();
        config.read_chunk = 0;
        assert!(Scenario::from_config(config).is_err());

        let mut config = base_config();
        config.chunk_size = Some(0);
        assert!(Scenario::from_config(config).is_err());
    }
}
