//! `tracemux` — single binary CLI / server.
//!
//! Subcommands: `serve | connect | send | watch | token-hash | detect | log | profile |
//! replay | extcap | import | export | ai-verify | json-schema`.

use clap::{Parser, Subcommand};
use tracemux_core::config::{migrate::migrate_config_to_latest, schema_v1::ConfigV1};
use tracemux_server::{
    run_with_session_root_classifier_encoding_pattern_startup_and_options as run_server_with_options,
    ServerRunOptions,
};

mod ai_verify;
mod cmd;
mod json_schema;

/// tracemux — unified terminal & log platform.
#[derive(Debug, Parser)]
#[command(name = "tracemux", version, about)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Debug, Subcommand)]
enum Cmd {
    /// Run the tracemux server (WSS).
    Serve(ServeArgs),
    /// Open a single channel and pipe to stdout.
    Connect(ConnectArgs),
    /// Send bytes to a running server session via WSS write-back.
    #[command(
        long_about = "Send bytes to a running server session via WSS write-back.\n\n\
        Payload precedence: --text (encoded via --encoding), then --file, then --hex; \
        if none are given the bytes are read from stdin. Use --udp-target host:port for \
        UDP sessions and --wait-ack to block until the server replies with ctl.write_ack \
        or ctl.error. Source-only transports (pcap, RTT, CAN) have no write path and will \
        reject writes."
    )]
    Send(SendArgs),
    /// Subscribe to a running server session and emit data frames as JSONL.
    #[command(
        long_about = "Subscribe to a running server session and emit data frames as JSONL.\n\n\
        One JSON object is printed per data frame. --encoding auto (default) projects binary \
        bodies using the server's per-source encoding snapshot; pass an explicit encoding \
        (e.g. shift_jis) to override. Use --max-frames to exit after a fixed count."
    )]
    Watch(WatchArgs),
    /// Hash a bearer token into argon2id PHC format for `serve --token-phc-file`.
    TokenHash(TokenHashArgs),
    /// Auto-detect available transports.
    #[command(long_about = "Auto-detect available transports.\n\n\
        v0.1 lists the statically known transport kinds and probes the host for serial-port \
        candidates only; TCP/UDP/process probes are placeholders for later releases. Use \
        --format json for a machine-readable report.")]
    Detect,
    /// Log a single channel to a session-dir.
    Log(LogArgs),
    /// Manage saved profiles.
    Profile(ProfileArgs),
    /// Replay an existing session-dir.
    #[command(long_about = "Replay an existing session-dir.\n\n\
        --rate is a wall-clock multiplier (2.0 = twice as fast); --rate 0 replays in lockstep \
        as fast as possible. --seed makes any jitter deterministic for reproducible runs.")]
    Replay(ReplayArgs),
    /// Wireshark extcap interface.
    #[command(long_about = "Wireshark extcap interface.\n\n\
        Implements the Wireshark extcap protocol modes (--extcap-interfaces, --extcap-dlts, \
        --extcap-config, --capture). Not intended to be run by hand; Wireshark invokes it. \
        Live packet capture additionally requires the optional `pcap-capture` build feature \
        and platform capture libraries (Npcap on Windows, libpcap on Unix).")]
    Extcap(ExtcapArgs),
    /// Import a foreign log artefact into a session-dir.
    #[command(long_about = "Import a foreign log artefact into a session-dir.\n\n\
        Supported kinds in v0.1: `text`, `csv`. `teraterm` and `pcapng` are reserved but \
        not yet implemented and will exit with a clear error. The destination must be empty \
        to avoid overwriting an existing session.")]
    Import(ImportArgs),
    /// Export a session-dir to a foreign format.
    #[command(long_about = "Export a session-dir to a foreign format.\n\n\
        Supported kinds: `csv`, `text`, `jsonl`, `pcapng`. Use --tz to format timestamps in a \
        fixed timezone (UTC, GMT+9, +09:00, Asia/Tokyo) and --encoding to decode raw text \
        bodies with a specific encoding instead of the session metadata.")]
    Export(ExportArgs),
    /// Run the aggregate AI verification gate.
    AiVerify,
    /// Emit JSON schemas for `--format json` output.
    JsonSchema(JsonSchemaArgs),
}

#[derive(Debug, clap::Args)]
#[allow(clippy::struct_excessive_bools)]
struct ServeArgs {
    /// Read server startup settings from a TOML config file.
    #[arg(long, value_name = "PATH")]
    config: Option<std::path::PathBuf>,
    /// Bind address (default 127.0.0.1:0 for auto, or config server.bind).
    #[arg(long)]
    bind: Option<String>,
    /// Root directory for server-created session-dirs.
    #[arg(long)]
    session_root: Option<std::path::PathBuf>,
    /// Session-dir name pattern using {prefix}, {kind}, {iface}, {timestamp}, {unix_ns}.
    #[arg(long, value_name = "PATTERN")]
    session_name_pattern: Option<String>,
    /// Disable auth (gated to loopback).
    #[arg(long, conflicts_with = "require_auth")]
    no_auth: bool,
    /// Require auth even when the config file disables it.
    #[arg(long, conflicts_with = "no_auth")]
    require_auth: bool,
    /// Add an argon2id PHC hash for an accepted bearer token.
    #[arg(long = "token-phc", value_name = "PHC")]
    token_phc: Vec<String>,
    /// Read accepted bearer-token PHC hashes from a file, one per line.
    #[arg(long = "token-phc-file", value_name = "PATH")]
    token_phc_files: Vec<std::path::PathBuf>,
    /// Serve HTTPS/WSS instead of HTTP/WS.
    #[arg(long)]
    tls: bool,
    /// Directory for TLS server.crt/server.key. Implies `--tls`.
    #[arg(long, value_name = "DIR")]
    tls_dir: Option<std::path::PathBuf>,
    /// Default text encoding for server-side decoded records.
    #[arg(long)]
    encoding: Option<String>,
    /// Content detection mode (`configured`, `auto`, `suggest`, `off`).
    #[arg(long = "detect-mode")]
    detect_mode: Option<String>,
    /// Add a server-side substring classifier as `contains=tag`.
    #[arg(long = "classify", value_name = "CONTAINS=TAG")]
    classify: Vec<String>,
    /// Add a server-side regex classifier as `regex=tag`.
    #[arg(long = "classify-regex", value_name = "REGEX=TAG")]
    classify_regex: Vec<String>,
    /// Detect and open every serial/COM port when the server starts.
    #[arg(long)]
    open_all_serial: bool,
    /// Explicit serial port(s) for `--open-all-serial`; repeat to open several.
    #[arg(long = "serial-port", value_name = "PORT")]
    serial_ports: Vec<String>,
    /// Baud rate used by `--open-all-serial`.
    #[arg(long)]
    serial_baud: Option<u32>,
    /// Data bits used by `--open-all-serial`.
    #[arg(long)]
    serial_data_bits: Option<u8>,
    /// Parity used by `--open-all-serial` (`none`, `even`, `odd`).
    #[arg(long)]
    serial_parity: Option<String>,
    /// Stop bits used by `--open-all-serial`.
    #[arg(long)]
    serial_stop_bits: Option<u8>,
    /// Flow control used by `--open-all-serial` (`none`, `hardware`, `software`).
    #[arg(long)]
    serial_flow: Option<String>,
}

#[derive(Debug, clap::Args)]
struct ConnectArgs {
    /// Channel spec, e.g. `serial://COM3?baud=115200`.
    spec: String,
    /// Also save received bytes into this session-dir.
    #[arg(long, value_name = "DIR")]
    save: Option<std::path::PathBuf>,
    /// Text encoding stored in metadata when `--save` is used.
    #[arg(long, default_value = "utf-8")]
    encoding: String,
}

#[derive(Debug, clap::Args)]
struct SendArgs {
    /// WebSocket endpoint for `tracemux serve`.
    #[arg(long, default_value = "ws://127.0.0.1:9000/ws")]
    url: String,
    /// Bearer token. Defaults to `TRACEMUX_TOKEN` when set.
    #[arg(long, env = "TRACEMUX_TOKEN")]
    token: Option<String>,
    /// Target session id.
    #[arg(long)]
    sid: String,
    /// Target channel.
    #[arg(long, default_value_t = 0)]
    ch: u32,
    /// UTF-8 text to send. If omitted with `--file`/`--hex`, stdin is read.
    #[arg(long)]
    text: Option<String>,
    /// Encoding for `--text` payloads (e.g. utf-8, shift_jis, cp932).
    #[arg(long, default_value = "utf-8")]
    encoding: String,
    /// File whose bytes should be sent.
    #[arg(long)]
    file: Option<std::path::PathBuf>,
    /// Hex bytes to send, e.g. `48656c6c6f0a`.
    #[arg(long)]
    hex: Option<String>,
    /// UDP destination `host:port` for UDP sessions.
    #[arg(long)]
    udp_target: Option<String>,
    /// Line ending appended to the payload: `none` (default), `cr`, `lf`, `crlf`.
    /// Use `crlf` for interactive cmd.exe, `lf` for POSIX shells.
    #[arg(long, default_value = "none")]
    newline: String,
    /// Wait for `ctl.write_ack` or `ctl.error` before exiting.
    #[arg(long)]
    wait_ack: bool,
}

#[derive(Debug, clap::Args)]
struct WatchArgs {
    /// WebSocket endpoint for `tracemux serve`.
    #[arg(long, default_value = "ws://127.0.0.1:9000/ws")]
    url: String,
    /// Bearer token. Defaults to `TRACEMUX_TOKEN` when set.
    #[arg(long, env = "TRACEMUX_TOKEN")]
    token: Option<String>,
    /// Target session id.
    #[arg(long)]
    sid: String,
    /// Target channel.
    #[arg(long, default_value_t = 0)]
    ch: u32,
    /// Encoding for binary body text projection (`auto` uses the server source snapshot).
    #[arg(long, default_value = "auto")]
    encoding: String,
    /// Exit after this many data frames.
    #[arg(long)]
    max_frames: Option<u64>,
}

#[derive(Debug, clap::Args)]
struct TokenHashArgs {
    /// Bearer token to hash. Prefer TRACEMUX_TOKEN over passing secrets on a command line.
    #[arg(long, env = "TRACEMUX_TOKEN")]
    token: String,
}

#[derive(Debug, clap::Args)]
struct LogArgs {
    /// Channel spec.
    spec: String,
    /// Output prefix (defaults to `tracemux`).
    #[arg(long)]
    prefix: Option<String>,
    /// Session-dir name pattern using {prefix}, {kind}, {iface}, {timestamp}, {unix_ns}.
    #[arg(long, value_name = "PATTERN")]
    name_pattern: Option<String>,
    /// Decode encoding used for `--classify` matching.
    #[arg(long, default_value = "utf-8")]
    encoding: String,
    /// Add a substring classifier as `contains=tag`.
    #[arg(long = "classify", value_name = "CONTAINS=TAG")]
    classify: Vec<String>,
    /// Add a regex classifier as `regex=tag`.
    #[arg(long = "classify-regex", value_name = "REGEX=TAG")]
    classify_regex: Vec<String>,
}

#[derive(Debug, clap::Args)]
struct ReplayArgs {
    /// Path to session-dir.
    #[arg(long)]
    session: std::path::PathBuf,
    /// Replay rate multiplier; 0 = lockstep.
    #[arg(long, default_value_t = 1.0)]
    rate: f32,
    /// Deterministic seed.
    #[arg(long)]
    seed: Option<u64>,
}

#[derive(Debug, clap::Args)]
struct ImportArgs {
    /// Importer kind: `text` and `csv` work; `teraterm` and `pcapng` are
    /// reserved but not implemented in v0.1 (they exit with an error).
    kind: String,
    /// Source artefact.
    src: std::path::PathBuf,
    /// Destination session-dir.
    dst: std::path::PathBuf,
}

#[derive(Debug, clap::Args)]
struct ExportArgs {
    /// Read export defaults from a TOML config file.
    #[arg(long, value_name = "PATH")]
    config: Option<std::path::PathBuf>,
    /// Exporter kind (`csv`, `text`, `jsonl`, `pcapng`).
    kind: String,
    /// Format exported timestamps in a fixed timezone (`UTC`, `GMT+9`, `+09:00`, `Asia/Tokyo`).
    #[arg(long)]
    tz: Option<String>,
    /// Decode raw text payloads with this encoding instead of session metadata.
    #[arg(long)]
    encoding: Option<String>,
    /// Source session-dir.
    src: std::path::PathBuf,
    /// Destination file.
    dst: std::path::PathBuf,
}

#[derive(Debug, clap::Args)]
struct ProfileArgs {
    /// Profile directory (defaults to platform config dir).
    #[arg(long)]
    dir: Option<std::path::PathBuf>,
    #[command(subcommand)]
    action: ProfileAction,
}

#[derive(Debug, clap::Subcommand)]
enum ProfileAction {
    /// List profile names.
    List,
    /// Show one profile.
    Show {
        /// Profile name.
        name: String,
    },
    /// Save a profile.
    Set {
        /// Profile name.
        name: String,
        /// Channel spec.
        spec: String,
    },
    /// Delete a profile.
    Del {
        /// Profile name.
        name: String,
    },
}

#[derive(Debug, clap::Args)]
#[allow(clippy::struct_excessive_bools)]
struct ExtcapArgs {
    /// Wireshark `--extcap-interfaces` mode.
    #[arg(long, group = "extcap_mode")]
    extcap_interfaces: bool,
    /// Wireshark `--extcap-dlts` mode.
    #[arg(long, group = "extcap_mode")]
    extcap_dlts: bool,
    /// Wireshark `--extcap-config` mode.
    #[arg(long, group = "extcap_mode")]
    extcap_config: bool,
    /// Wireshark `--capture` mode.
    #[arg(long, group = "extcap_mode")]
    capture: bool,
    /// Selected interface (from `--extcap-interface NAME`).
    #[arg(long)]
    extcap_interface: Option<String>,
    /// Capture FIFO path (from `--fifo PATH`).
    #[arg(long)]
    fifo: Option<String>,
    /// Channel spec URI (forwarded to `--capture` mode).
    #[arg(long)]
    spec: Option<String>,
}

#[derive(Debug, clap::Args)]
struct JsonSchemaArgs {
    /// Output directory.
    #[arg(long, default_value = "docs/protocols/cli-output/v1")]
    out: std::path::PathBuf,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();
    run_cmd(cli.cmd).await
}

async fn run_cmd(cmd: Cmd) -> anyhow::Result<()> {
    match cmd {
        Cmd::Serve(args) => run_serve(args).await?,
        Cmd::Connect(args) => {
            cmd::connect::run(cmd::connect::Options {
                spec: args.spec,
                save: args.save,
                encoding: args.encoding,
            })
            .await?;
        }
        Cmd::Send(args) => run_send(args).await?,
        Cmd::Watch(args) => run_watch(args).await?,
        Cmd::TokenHash(args) => run_token_hash(&args)?,
        Cmd::Detect => cmd::detect::run()?,
        Cmd::Log(args) => run_log(args).await?,
        Cmd::Profile(args) => run_profile(args)?,
        Cmd::Replay(args) => {
            tracemux_replay::run(&args.session, args.rate, args.seed).await?;
        }
        Cmd::Extcap(args) => run_extcap(args).await?,
        Cmd::Import(args) => cmd::import::run(&args.kind, &args.src, &args.dst).await?,
        Cmd::Export(args) => {
            let config = load_serve_config(args.config.as_deref())?;
            let timezone = export_timezone(&args, config.as_ref());
            let encoding = export_encoding(&args, config.as_ref());
            cmd::export::run(
                &args.kind,
                &args.src,
                &args.dst,
                timezone.as_deref(),
                encoding.as_deref(),
            )?;
        }
        Cmd::AiVerify => ai_verify::run().await?,
        Cmd::JsonSchema(args) => json_schema::emit(&args.out)?,
    }
    Ok(())
}

async fn run_serve(args: ServeArgs) -> anyhow::Result<()> {
    let config = load_serve_config(args.config.as_deref())?;
    let classifier = cmd::log::classifier_from_specs(&args.classify, &args.classify_regex)?;
    let detect_mode = serve_detect_mode(&args, config.as_ref());
    let detection_mode = tracemux_core::detect::content::DetectionMode::parse(&detect_mode)
        .ok_or_else(|| {
            anyhow::anyhow!("--detect-mode must be configured, auto, suggest, or off")
        })?;
    let startup = serve_startup_sources(&args, config.as_ref());
    let bind = serve_bind(&args, config.as_ref());
    let session_root = serve_session_root(&args, config.as_ref());
    let security = serve_security(&args, config.as_ref(), &session_root);
    let no_auth = serve_no_auth(&args, config.as_ref());
    let encoding = serve_encoding(&args, config.as_ref());
    let session_name_pattern = serve_session_name_pattern(&args, config.as_ref());
    let retention_keep_days = serve_retention_keep_days(config.as_ref());
    let export_defaults = serve_export_defaults(config.as_ref());
    let ws_delivery = serve_ws_delivery(config.as_ref());
    run_server_with_options(
        &bind,
        no_auth,
        session_root,
        classifier,
        encoding,
        session_name_pattern.unwrap_or_else(|| {
            tracemux_core::session_name::DEFAULT_SERVER_SESSION_NAME_PATTERN.to_string()
        }),
        startup,
        ServerRunOptions {
            detection_mode,
            security,
            retention_keep_days,
            export_defaults,
            ws_delivery,
        },
    )
    .await
}

fn load_serve_config(path: Option<&std::path::Path>) -> anyhow::Result<Option<ConfigV1>> {
    let Some(path) = path else {
        return Ok(None);
    };
    let body = std::fs::read_to_string(path)
        .map_err(|err| anyhow::anyhow!("reading config {}: {err}", path.display()))?;
    let config = migrate_config_to_latest(&body)
        .map_err(|err| anyhow::anyhow!("parsing config {}: {err}", path.display()))?;
    Ok(Some(config))
}

fn serve_bind(args: &ServeArgs, config: Option<&ConfigV1>) -> String {
    args.bind
        .clone()
        .or_else(|| config.map(|c| c.server.bind.clone()))
        .unwrap_or_else(|| "127.0.0.1:0".to_string())
}

fn serve_session_root(args: &ServeArgs, config: Option<&ConfigV1>) -> std::path::PathBuf {
    args.session_root
        .clone()
        .or_else(|| config.map(|c| std::path::PathBuf::from(&c.server.session_root)))
        .unwrap_or_else(|| std::path::PathBuf::from("tracemux-sessions"))
}

fn serve_encoding(args: &ServeArgs, config: Option<&ConfigV1>) -> String {
    args.encoding
        .clone()
        .or_else(|| config.map(|c| c.server.encoding.clone()))
        .unwrap_or_else(|| "utf-8".to_string())
}

fn serve_detect_mode(args: &ServeArgs, config: Option<&ConfigV1>) -> String {
    args.detect_mode
        .clone()
        .or_else(|| config.map(|c| c.server.detect_mode.clone()))
        .unwrap_or_else(|| "configured".to_string())
}

fn serve_session_name_pattern(args: &ServeArgs, config: Option<&ConfigV1>) -> Option<String> {
    args.session_name_pattern
        .clone()
        .or_else(|| config.and_then(|c| c.server.session_name_pattern.clone()))
}

fn serve_retention_keep_days(config: Option<&ConfigV1>) -> u32 {
    config.map_or(0, |c| c.retention.keep_days)
}

fn serve_ws_delivery(config: Option<&ConfigV1>) -> tracemux_server::ws::WsDeliveryOptions {
    tracemux_server::ws::WsDeliveryOptions {
        min_send_interval: std::time::Duration::from_millis(
            config.map_or(0, |c| c.ui.live_flush_ms),
        ),
    }
}

fn serve_export_defaults(config: Option<&ConfigV1>) -> tracemux_server::export_api::ExportDefaults {
    tracemux_server::export_api::ExportDefaults {
        timezone: config.and_then(|c| c.export.timezone.clone()),
        encoding: config.and_then(|c| c.export.encoding.clone()),
    }
}

fn export_timezone(args: &ExportArgs, config: Option<&ConfigV1>) -> Option<String> {
    args.tz
        .clone()
        .or_else(|| config.and_then(|c| c.export.timezone.clone()))
}

fn export_encoding(args: &ExportArgs, config: Option<&ConfigV1>) -> Option<String> {
    args.encoding
        .clone()
        .or_else(|| config.and_then(|c| c.export.encoding.clone()))
}

fn serve_no_auth(args: &ServeArgs, config: Option<&ConfigV1>) -> bool {
    if args.no_auth {
        return true;
    }
    if args.require_auth {
        return false;
    }
    config.is_some_and(|c| !c.server.require_auth)
}

async fn run_send(args: SendArgs) -> anyhow::Result<()> {
    cmd::send::run(cmd::send::Options {
        url: args.url,
        token: args.token,
        sid: args.sid,
        ch: args.ch,
        text: args.text,
        encoding: args.encoding,
        file: args.file,
        hex: args.hex,
        udp_target: args.udp_target,
        newline: args.newline,
        wait_ack: args.wait_ack,
    })
    .await
}

async fn run_watch(args: WatchArgs) -> anyhow::Result<()> {
    cmd::watch::run(cmd::watch::Options {
        url: args.url,
        token: args.token,
        sid: args.sid,
        ch: args.ch,
        encoding: args.encoding,
        max_frames: args.max_frames,
    })
    .await
}

fn run_token_hash(args: &TokenHashArgs) -> anyhow::Result<()> {
    if args.token.is_empty() {
        anyhow::bail!("token must not be empty");
    }
    println!("{}", tracemux_server::auth::hash_token(&args.token)?);
    Ok(())
}

async fn run_log(args: LogArgs) -> anyhow::Result<()> {
    cmd::log::run(cmd::log::Options {
        spec: args.spec,
        prefix: args.prefix,
        name_pattern: args.name_pattern,
        encoding: args.encoding,
        classify: args.classify,
        classify_regex: args.classify_regex,
    })
    .await
}

fn run_profile(args: ProfileArgs) -> anyhow::Result<()> {
    let dir = args.dir.unwrap_or_else(cmd::profile::default_dir);
    let action = match args.action {
        ProfileAction::List => cmd::profile::Action::List,
        ProfileAction::Show { name } => cmd::profile::Action::Show { name },
        ProfileAction::Set { name, spec } => cmd::profile::Action::Set { name, spec },
        ProfileAction::Del { name } => cmd::profile::Action::Del { name },
    };
    cmd::profile::run(&dir, action)
}

async fn run_extcap(args: ExtcapArgs) -> anyhow::Result<()> {
    cmd::extcap::run(extcap_mode(args)?).await
}

fn extcap_mode(args: ExtcapArgs) -> anyhow::Result<cmd::extcap::Mode> {
    if args.extcap_interfaces {
        return Ok(cmd::extcap::Mode::Interfaces);
    }
    if args.extcap_dlts {
        return Ok(cmd::extcap::Mode::Dlts {
            interface: args
                .extcap_interface
                .ok_or_else(|| anyhow::anyhow!("--extcap-dlts requires --extcap-interface"))?,
        });
    }
    if args.extcap_config {
        return Ok(cmd::extcap::Mode::Config {
            interface: args
                .extcap_interface
                .ok_or_else(|| anyhow::anyhow!("--extcap-config requires --extcap-interface"))?,
        });
    }
    if args.capture {
        return Ok(cmd::extcap::Mode::Capture {
            interface: args
                .extcap_interface
                .ok_or_else(|| anyhow::anyhow!("--capture requires --extcap-interface"))?,
            fifo: args
                .fifo
                .ok_or_else(|| anyhow::anyhow!("--capture requires --fifo"))?,
            spec: args
                .spec
                .ok_or_else(|| anyhow::anyhow!("--capture requires --spec"))?,
        });
    }
    anyhow::bail!(
        "extcap: one of --extcap-interfaces / --extcap-dlts / --extcap-config / --capture is required"
    );
}

fn serve_startup_sources(
    args: &ServeArgs,
    config: Option<&ConfigV1>,
) -> tracemux_server::StartupSources {
    let mut startup = tracemux_server::StartupSources::default();
    if let Some(config) = config {
        startup.channels = config
            .channels
            .iter()
            .map(|(name, channel)| tracemux_server::StartupChannel {
                name: name.clone(),
                label: channel.label.clone(),
                spec: channel.spec.clone(),
                local_echo: channel.local_echo.clone(),
                newline: channel.newline.clone(),
            })
            .collect();
    }
    let config_serial = config.map(|c| &c.server.serial);
    if args.open_all_serial || config_serial.is_some_and(|serial| serial.open_all) {
        let ports = if args.serial_ports.is_empty() {
            config_serial
                .and_then(|serial| (!serial.ports.is_empty()).then(|| serial.ports.clone()))
        } else {
            Some(args.serial_ports.clone())
        };
        startup.serial = Some(tracemux_server::SerialAutostart {
            ports,
            options: tracemux_server::source_manager::SerialPortOptions {
                baud: args
                    .serial_baud
                    .or_else(|| config_serial.map(|serial| serial.baud))
                    .unwrap_or(115_200),
                data_bits: args
                    .serial_data_bits
                    .or_else(|| config_serial.map(|serial| serial.data_bits))
                    .unwrap_or(8),
                parity: args
                    .serial_parity
                    .clone()
                    .or_else(|| config_serial.map(|serial| serial.parity.clone()))
                    .unwrap_or_else(|| "none".to_string()),
                stop_bits: args
                    .serial_stop_bits
                    .or_else(|| config_serial.map(|serial| serial.stop_bits))
                    .unwrap_or(1),
                flow: args
                    .serial_flow
                    .clone()
                    .or_else(|| config_serial.map(|serial| serial.flow.clone()))
                    .unwrap_or_else(|| "none".to_string()),
            },
        });
    }
    startup
}

fn serve_security(
    args: &ServeArgs,
    config: Option<&ConfigV1>,
    session_root: &std::path::Path,
) -> tracemux_server::ServerSecurity {
    let config_tls_dir = config
        .and_then(|c| c.server.tls.dir.as_ref())
        .map(std::path::PathBuf::from);
    let config_tls_enabled = config.is_some_and(|c| c.server.tls.enabled);
    let tls =
        if args.tls || args.tls_dir.is_some() || config_tls_enabled || config_tls_dir.is_some() {
            Some(tracemux_server::TlsServeConfig {
                dir: args
                    .tls_dir
                    .clone()
                    .or(config_tls_dir)
                    .unwrap_or_else(|| session_root.join("tls")),
            })
        } else {
            None
        };
    let mut token_phc_files: Vec<std::path::PathBuf> = config
        .map(|c| {
            c.server
                .token_phc_files
                .iter()
                .map(std::path::PathBuf::from)
                .collect()
        })
        .unwrap_or_default();
    token_phc_files.extend(args.token_phc_files.clone());
    tracemux_server::ServerSecurity {
        token_phc: args.token_phc.clone(),
        token_phc_files,
        tls,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serve_open_all_serial_args_build_startup_config() {
        // REQ: FR-CLI-008
        let cli = Cli::try_parse_from([
            "tracemux",
            "serve",
            "--open-all-serial",
            "--serial-port",
            "COM7",
            "--serial-port",
            "COM8",
            "--serial-baud",
            "9600",
            "--serial-data-bits",
            "7",
            "--serial-parity",
            "even",
            "--serial-stop-bits",
            "2",
            "--serial-flow",
            "hardware",
        ])
        .unwrap();

        let Cmd::Serve(args) = cli.cmd else {
            panic!("expected serve command");
        };
        let startup = serve_startup_sources(&args, None);
        let serial = startup.serial.expect("serial startup");
        assert_eq!(
            serial.ports,
            Some(vec!["COM7".to_string(), "COM8".to_string()])
        );
        assert_eq!(serial.options.baud, 9_600);
        assert_eq!(serial.options.data_bits, 7);
        assert_eq!(serial.options.parity, "even");
        assert_eq!(serial.options.stop_bits, 2);
        assert_eq!(serial.options.flow, "hardware");
    }

    #[test]
    fn serve_security_args_build_security_config() {
        // REQ: FR-WIRE-002
        let cli = Cli::try_parse_from([
            "tracemux",
            "serve",
            "--session-root",
            "sessions",
            "--token-phc",
            "$argon2id$v=19$m=1,t=1,p=1$c2FsdA$uUuXQDV5uH1o0kQ8qM1S7g",
            "--token-phc-file",
            "tokens.phc",
            "--tls-dir",
            "tls-state",
        ])
        .unwrap();

        let Cmd::Serve(args) = cli.cmd else {
            panic!("expected serve command");
        };
        let session_root = serve_session_root(&args, None);
        let security = serve_security(&args, None, &session_root);
        assert_eq!(security.token_phc.len(), 1);
        assert_eq!(
            security.token_phc_files,
            vec![std::path::PathBuf::from("tokens.phc")]
        );
        assert_eq!(
            security.tls.expect("tls config").dir,
            std::path::PathBuf::from("tls-state")
        );
    }

    #[test]
    fn serve_config_file_supplies_bind_auth_and_channels() {
        // REQ: FR-CLI-012
        let path = write_temp_config(
            r#"
                config_version = 1
                [server]
                bind = "127.0.0.1:9443"
                session_root = "sessions-from-config"
                encoding = "shift_jis"
                detect_mode = "suggest"
                session_name_pattern = "{prefix}-{kind}-{iface}-cfg"
                token_phc_files = ["tokens-from-config.phc"]
                require_auth = false

                [server.serial]
                open_all = true
                ports = ["COM9"]
                baud = 57600
                data_bits = 7
                parity = "odd"
                stop_bits = 2
                flow = "software"

                [server.tls]
                enabled = true
                dir = "tls-from-config"

                [export]
                timezone = "Asia/Tokyo"
                encoding = "utf-8"

                [ui]
                live_flush_ms = 25

                [retention]
                keep_days = 7

                [channels.demo]
                label = "demo source"
                [channels.demo.spec]
                kind = "mock"
                tag = "demo"
            "#,
        );
        let path_arg = path.to_string_lossy().to_string();
        let cli =
            Cli::try_parse_from(["tracemux", "serve", "--config", path_arg.as_str()]).unwrap();

        let Cmd::Serve(args) = cli.cmd else {
            panic!("expected serve command");
        };
        let config = load_serve_config(args.config.as_deref()).unwrap().unwrap();
        assert_eq!(serve_bind(&args, Some(&config)), "127.0.0.1:9443");
        assert_eq!(
            serve_session_root(&args, Some(&config)),
            std::path::PathBuf::from("sessions-from-config")
        );
        assert_eq!(serve_encoding(&args, Some(&config)), "shift_jis");
        assert_eq!(serve_detect_mode(&args, Some(&config)), "suggest");
        assert_eq!(
            serve_session_name_pattern(&args, Some(&config)).as_deref(),
            Some("{prefix}-{kind}-{iface}-cfg")
        );
        assert_eq!(serve_retention_keep_days(Some(&config)), 7);
        assert_eq!(
            serve_export_defaults(Some(&config)).timezone.as_deref(),
            Some("Asia/Tokyo")
        );
        assert_eq!(
            serve_export_defaults(Some(&config)).encoding.as_deref(),
            Some("utf-8")
        );
        assert_eq!(
            serve_ws_delivery(Some(&config)).min_send_interval,
            std::time::Duration::from_millis(25)
        );
        assert!(serve_no_auth(&args, Some(&config)));
        let session_root = serve_session_root(&args, Some(&config));
        let security = serve_security(&args, Some(&config), &session_root);
        assert_eq!(
            security.token_phc_files,
            vec![std::path::PathBuf::from("tokens-from-config.phc")]
        );
        assert_eq!(
            security.tls.expect("tls config").dir,
            std::path::PathBuf::from("tls-from-config")
        );

        let startup = serve_startup_sources(&args, Some(&config));
        assert_eq!(startup.channels.len(), 1);
        let serial = startup.serial.expect("serial startup");
        assert_eq!(serial.ports, Some(vec!["COM9".to_string()]));
        assert_eq!(serial.options.baud, 57_600);
        assert_eq!(serial.options.data_bits, 7);
        assert_eq!(serial.options.parity, "odd");
        assert_eq!(serial.options.stop_bits, 2);
        assert_eq!(serial.options.flow, "software");
        assert_eq!(startup.channels[0].name, "demo");
        assert_eq!(startup.channels[0].label.as_deref(), Some("demo source"));
        match &startup.channels[0].spec {
            tracemux_core::source::ChannelSpec::Mock { tag } => assert_eq!(tag, "demo"),
            other => panic!("wrong channel spec: {other:?}"),
        }
    }

    #[test]
    fn serve_cli_overrides_config_bind_and_auth() {
        // REQ: FR-CLI-012
        let path = write_temp_config(
            r#"
                config_version = 1
                [server]
                bind = "127.0.0.1:9443"
                session_root = "sessions-from-config"
                encoding = "shift_jis"
                detect_mode = "suggest"
                session_name_pattern = "{prefix}-{kind}-{iface}-cfg"
                token_phc_files = ["tokens-from-config.phc"]
                require_auth = false

                [server.serial]
                open_all = true
                ports = ["COM9"]
                baud = 57600
                data_bits = 7
                parity = "odd"
                stop_bits = 2
                flow = "software"

                [server.tls]
                enabled = true
                dir = "tls-from-config"
            "#,
        );
        let path_arg = path.to_string_lossy().to_string();
        let cli = Cli::try_parse_from([
            "tracemux",
            "serve",
            "--config",
            path_arg.as_str(),
            "--bind",
            "127.0.0.1:7777",
            "--session-root",
            "sessions-from-cli",
            "--encoding",
            "cp932",
            "--detect-mode",
            "off",
            "--session-name-pattern",
            "{prefix}-{kind}-cli",
            "--serial-port",
            "COM10",
            "--serial-baud",
            "115200",
            "--serial-parity",
            "none",
            "--token-phc-file",
            "tokens-from-cli.phc",
            "--tls-dir",
            "tls-from-cli",
            "--require-auth",
        ])
        .unwrap();

        let Cmd::Serve(args) = cli.cmd else {
            panic!("expected serve command");
        };
        let config = load_serve_config(args.config.as_deref()).unwrap().unwrap();
        assert_eq!(serve_bind(&args, Some(&config)), "127.0.0.1:7777");
        assert_eq!(
            serve_session_root(&args, Some(&config)),
            std::path::PathBuf::from("sessions-from-cli")
        );
        assert_eq!(serve_encoding(&args, Some(&config)), "cp932");
        assert_eq!(serve_detect_mode(&args, Some(&config)), "off");
        assert_eq!(
            serve_session_name_pattern(&args, Some(&config)).as_deref(),
            Some("{prefix}-{kind}-cli")
        );
        assert!(!serve_no_auth(&args, Some(&config)));
        let session_root = serve_session_root(&args, Some(&config));
        let security = serve_security(&args, Some(&config), &session_root);
        assert_eq!(
            security.token_phc_files,
            vec![
                std::path::PathBuf::from("tokens-from-config.phc"),
                std::path::PathBuf::from("tokens-from-cli.phc")
            ]
        );
        assert_eq!(
            security.tls.expect("tls config").dir,
            std::path::PathBuf::from("tls-from-cli")
        );
        let startup = serve_startup_sources(&args, Some(&config));
        let serial = startup.serial.expect("serial startup");
        assert_eq!(serial.ports, Some(vec!["COM10".to_string()]));
        assert_eq!(serial.options.baud, 115_200);
        assert_eq!(serial.options.data_bits, 7);
        assert_eq!(serial.options.parity, "none");
        assert_eq!(serial.options.stop_bits, 2);
        assert_eq!(serial.options.flow, "software");
    }

    #[test]
    fn export_config_supplies_timezone_and_encoding_defaults() {
        // REQ: FR-CLI-012
        let path = write_temp_config(
            r#"
                config_version = 1
                [export]
                timezone = "Asia/Tokyo"
                encoding = "shift_jis"
            "#,
        );
        let path_arg = path.to_string_lossy().to_string();
        let cli = Cli::try_parse_from([
            "tracemux",
            "export",
            "--config",
            path_arg.as_str(),
            "text",
            "src-session",
            "out.txt",
        ])
        .unwrap();

        let Cmd::Export(args) = cli.cmd else {
            panic!("expected export command");
        };
        let config = load_serve_config(args.config.as_deref()).unwrap().unwrap();
        assert_eq!(
            export_timezone(&args, Some(&config)).as_deref(),
            Some("Asia/Tokyo")
        );
        assert_eq!(
            export_encoding(&args, Some(&config)).as_deref(),
            Some("shift_jis")
        );
    }

    #[test]
    fn serve_config_rejects_unknown_version() {
        // REQ: FR-CLI-012
        let path = write_temp_config(
            r#"
                config_version = 2
                [server]
                bind = "127.0.0.1:9443"
                require_auth = true
            "#,
        );

        let err = load_serve_config(Some(&path)).unwrap_err();
        assert!(err.to_string().contains("unsupported config_version 2"));
    }

    #[test]
    fn token_hash_args_parse_token() {
        let cli = Cli::try_parse_from(["tracemux", "token-hash", "--token", "secret"]).unwrap();
        let Cmd::TokenHash(args) = cli.cmd else {
            panic!("expected token-hash command");
        };
        assert_eq!(args.token, "secret");
    }

    fn write_temp_config(body: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "tracemux-cli-config-{}-{}",
            std::process::id(),
            tracemux_core::time::unix_ns_now()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tracemux.toml");
        std::fs::write(&path, body).unwrap();
        path
    }
}
