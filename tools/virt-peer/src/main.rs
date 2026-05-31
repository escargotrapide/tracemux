//! `tracemux-virt-peer` ? a virtual counterparty for development and tests.
//!
//! The tool can behave like a small device over TCP or an already-created
//! virtual/physical serial port. It intentionally does not create Windows COM
//! devices by itself; use a virtual COM pair driver such as com0com for that.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};
use scenario::{Eol, Scenario, ScenarioConfig};
use tracing_subscriber::EnvFilter;
use transcript::Transcript;
use transport::serial;
use transport::tcp;

mod scenario;
mod transcript;
mod transport;

/// Command-line interface.
#[derive(Debug, Parser)]
#[command(name = "tracemux-virt-peer", version, about)]
struct Cli {
    /// Logging filter, for example `info` or `debug,tracemux_virt_peer=trace`.
    #[arg(long, global = true, default_value = "info")]
    log_filter: String,
    #[command(subcommand)]
    command: Command,
}

/// Supported virtual-peer transports.
#[derive(Debug, Subcommand)]
enum Command {
    /// Run as a TCP peer.
    Tcp(TcpArgs),
    /// Run against an existing serial/COM port.
    Serial(SerialArgs),
}

/// TCP transport options.
#[derive(Debug, Args)]
struct TcpArgs {
    /// TCP mode: listen for tracemux, or connect to an existing listener.
    #[arg(long, value_enum, default_value_t = TcpModeArg::Listen)]
    mode: TcpModeArg,
    /// Address to bind/connect. Use port 0 in listen mode for an OS-assigned port.
    #[arg(long, default_value = "127.0.0.1:0")]
    addr: String,
    #[command(flatten)]
    scenario: ScenarioArgs,
}

/// TCP mode argument.
#[derive(Debug, Clone, Copy, ValueEnum)]
enum TcpModeArg {
    /// Bind a listener and accept one connection.
    Listen,
    /// Connect to an existing TCP listener.
    Connect,
}

impl From<TcpModeArg> for tcp::Mode {
    fn from(value: TcpModeArg) -> Self {
        match value {
            TcpModeArg::Listen => Self::Listen,
            TcpModeArg::Connect => Self::Connect,
        }
    }
}

/// Serial transport options.
#[derive(Debug, Args)]
struct SerialArgs {
    /// COM port/device path, for example `COM21` or `/dev/ttyUSB0`.
    #[arg(long)]
    port: String,
    /// Baud rate.
    #[arg(long, default_value_t = 115_200)]
    baud: u32,
    /// Data bits (5, 6, 7, or 8).
    #[arg(long, default_value_t = 8)]
    data_bits: u8,
    /// Parity.
    #[arg(long, value_enum, default_value_t = ParityArg::None)]
    parity: ParityArg,
    /// Stop bits.
    #[arg(long, value_enum, default_value_t = StopBitsArg::One)]
    stop_bits: StopBitsArg,
    /// Flow control.
    #[arg(long, value_enum, default_value_t = FlowArg::None)]
    flow: FlowArg,
    /// Blocking read timeout used by the serial driver.
    #[arg(long, default_value_t = 100)]
    read_timeout_ms: u64,
    #[command(flatten)]
    scenario: ScenarioArgs,
}

/// Serial parity argument.
#[derive(Debug, Clone, Copy, ValueEnum)]
enum ParityArg {
    /// No parity bit.
    None,
    /// Even parity.
    Even,
    /// Odd parity.
    Odd,
}

impl ParityArg {
    const fn as_token(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Even => "even",
            Self::Odd => "odd",
        }
    }
}

/// Serial stop-bits argument.
#[derive(Debug, Clone, Copy, ValueEnum)]
enum StopBitsArg {
    /// One stop bit.
    One,
    /// Two stop bits.
    Two,
}

impl StopBitsArg {
    const fn as_u8(self) -> u8 {
        match self {
            Self::One => 1,
            Self::Two => 2,
        }
    }
}

/// Serial flow-control argument.
#[derive(Debug, Clone, Copy, ValueEnum)]
enum FlowArg {
    /// No flow control.
    None,
    /// RTS/CTS hardware flow control.
    Hardware,
    /// XON/XOFF software flow control.
    Software,
}

impl FlowArg {
    const fn as_token(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Hardware => "hardware",
            Self::Software => "software",
        }
    }
}

/// Shared scripted-device behavior options.
#[derive(Debug, Clone, Args)]
struct ScenarioArgs {
    /// Text payload to send. May be repeated.
    #[arg(long = "send", value_name = "TEXT")]
    send_text: Vec<String>,
    /// Hex payload to send, for example `48656c6c6f0a`.
    #[arg(long = "send-hex", value_name = "HEX")]
    send_hex: Vec<String>,
    /// Number of times to repeat all configured payloads. Zero means no scripted sends.
    #[arg(long, default_value_t = 1)]
    repeat: u32,
    /// Delay before the first scripted payload.
    #[arg(long, default_value_t = 0)]
    initial_delay_ms: u64,
    /// Delay between scripted payloads.
    #[arg(long, default_value_t = 1_000)]
    interval_ms: u64,
    /// Line ending appended to text payloads.
    #[arg(long, value_enum, default_value_t = EolArg::None)]
    eol: EolArg,
    /// Optional chunk size for splitting outbound payloads.
    #[arg(long)]
    chunk_size: Option<usize>,
    /// Reply to inbound bytes with `ack-prefix + inbound`.
    #[arg(long)]
    echo: bool,
    /// Prefix used when `--echo` is enabled.
    #[arg(long, default_value = "ACK:")]
    ack_prefix: String,
    /// Read buffer size.
    #[arg(long, default_value_t = 4_096)]
    read_chunk: usize,
    /// Exit after this many idle milliseconds without inbound bytes.
    #[arg(long)]
    idle_timeout_ms: Option<u64>,
    /// Optional JSONL transcript path.
    #[arg(long)]
    transcript: Option<PathBuf>,
}

impl ScenarioArgs {
    fn into_scenario(self) -> Result<Scenario> {
        Scenario::from_config(ScenarioConfig {
            send_text: self.send_text,
            send_hex: self.send_hex,
            repeat: self.repeat,
            initial_delay_ms: self.initial_delay_ms,
            interval_ms: self.interval_ms,
            eol: self.eol.into(),
            chunk_size: self.chunk_size,
            echo: self.echo,
            ack_prefix: self.ack_prefix,
            read_chunk: self.read_chunk,
            idle_timeout_ms: self.idle_timeout_ms,
        })
    }
}

/// Line-ending CLI argument.
#[derive(Debug, Clone, Copy, ValueEnum)]
enum EolArg {
    /// Append nothing.
    None,
    /// Append LF.
    Lf,
    /// Append CRLF.
    Crlf,
}

impl From<EolArg> for Eol {
    fn from(value: EolArg) -> Self {
        match value {
            EolArg::None => Self::None,
            EolArg::Lf => Self::Lf,
            EolArg::Crlf => Self::Crlf,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    init_tracing(&cli.log_filter)?;
    match cli.command {
        Command::Tcp(args) => run_tcp(args).await,
        Command::Serial(args) => run_serial(args).await,
    }
}

fn init_tracing(filter: &str) -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_new(filter).context("invalid log filter")?)
        .try_init()
        .map_err(|e| anyhow::anyhow!("initialising tracing: {e}"))
}

async fn run_tcp(args: TcpArgs) -> Result<()> {
    let TcpArgs {
        mode,
        addr,
        scenario,
    } = args;
    let transcript = Arc::new(Transcript::open(scenario.transcript.as_deref())?);
    let scenario = scenario.into_scenario()?;
    tcp::run(
        tcp::Config {
            mode: mode.into(),
            addr,
        },
        scenario,
        transcript,
    )
    .await
}

async fn run_serial(args: SerialArgs) -> Result<()> {
    let SerialArgs {
        port,
        baud,
        data_bits,
        parity,
        stop_bits,
        flow,
        read_timeout_ms,
        scenario,
    } = args;
    let transcript = Arc::new(Transcript::open(scenario.transcript.as_deref())?);
    let scenario = scenario.into_scenario()?;
    serial::run(
        serial::Config {
            port,
            baud,
            data_bits,
            parity: parity.as_token().to_string(),
            stop_bits: stop_bits.as_u8(),
            flow: flow.as_token().to_string(),
            read_timeout_ms,
        },
        scenario,
        transcript,
    )
    .await
}
