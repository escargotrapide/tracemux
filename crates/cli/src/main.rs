//! `wanlogger` — single binary CLI / server.
//!
//! Subcommands: `serve | connect | send | watch | token-hash | detect | log | profile |
//! replay | extcap | import | export | ai-verify | json-schema`.

use clap::{Parser, Subcommand};

mod ai_verify;
mod cmd;
mod json_schema;

/// wanlogger — unified terminal & log platform.
#[derive(Debug, Parser)]
#[command(name = "wanlogger", version, about)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Debug, Subcommand)]
enum Cmd {
    /// Run the wanlogger server (WSS).
    Serve(ServeArgs),
    /// Open a single channel and pipe to stdout.
    Connect(ConnectArgs),
    /// Send bytes to a running server session via WSS write-back.
    Send(SendArgs),
    /// Subscribe to a running server session and emit data frames as JSONL.
    Watch(WatchArgs),
    /// Hash a bearer token into argon2id PHC format for `serve --token-phc-file`.
    TokenHash(TokenHashArgs),
    /// Auto-detect available transports.
    Detect,
    /// Log a single channel to a session-dir.
    Log(LogArgs),
    /// Manage saved profiles.
    Profile(ProfileArgs),
    /// Replay an existing session-dir.
    Replay(ReplayArgs),
    /// Wireshark extcap interface.
    Extcap(ExtcapArgs),
    /// Import a foreign log artefact into a session-dir.
    Import(ImportArgs),
    /// Export a session-dir to a foreign format.
    Export(ExportArgs),
    /// Run the aggregate AI verification gate.
    AiVerify,
    /// Emit JSON schemas for `--format json` output.
    JsonSchema(JsonSchemaArgs),
}

#[derive(Debug, clap::Args)]
struct ServeArgs {
    /// Bind address (default 127.0.0.1:0 for auto).
    #[arg(long, default_value = "127.0.0.1:0")]
    bind: String,
    /// Root directory for server-created session-dirs.
    #[arg(long, default_value = "wanlogger-sessions")]
    session_root: std::path::PathBuf,
    /// Session-dir name pattern using {prefix}, {kind}, {iface}, {timestamp}, {unix_ns}.
    #[arg(long, value_name = "PATTERN")]
    session_name_pattern: Option<String>,
    /// Disable auth (gated to loopback).
    #[arg(long)]
    no_auth: bool,
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
    #[arg(long, default_value = "utf-8")]
    encoding: String,
    /// Add a server-side substring classifier as `contains=tag`.
    #[arg(long = "classify", value_name = "CONTAINS=TAG")]
    classify: Vec<String>,
    /// Detect and open every serial/COM port when the server starts.
    #[arg(long)]
    open_all_serial: bool,
    /// Explicit serial port(s) for `--open-all-serial`; repeat to open several.
    #[arg(long = "serial-port", value_name = "PORT")]
    serial_ports: Vec<String>,
    /// Baud rate used by `--open-all-serial`.
    #[arg(long, default_value_t = 115_200)]
    serial_baud: u32,
    /// Data bits used by `--open-all-serial`.
    #[arg(long, default_value_t = 8)]
    serial_data_bits: u8,
    /// Parity used by `--open-all-serial` (`none`, `even`, `odd`).
    #[arg(long, default_value = "none")]
    serial_parity: String,
    /// Stop bits used by `--open-all-serial`.
    #[arg(long, default_value_t = 1)]
    serial_stop_bits: u8,
    /// Flow control used by `--open-all-serial` (`none`, `hardware`, `software`).
    #[arg(long, default_value = "none")]
    serial_flow: String,
}

#[derive(Debug, clap::Args)]
struct ConnectArgs {
    /// Channel spec, e.g. `serial://COM3?baud=115200`.
    spec: String,
}

#[derive(Debug, clap::Args)]
struct SendArgs {
    /// WebSocket endpoint for `wanlogger serve`.
    #[arg(long, default_value = "ws://127.0.0.1:9000/ws")]
    url: String,
    /// Bearer token. Defaults to `WANLOGGER_TOKEN` when set.
    #[arg(long, env = "WANLOGGER_TOKEN")]
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
    /// Wait for `ctl.write_ack` or `ctl.error` before exiting.
    #[arg(long)]
    wait_ack: bool,
}

#[derive(Debug, clap::Args)]
struct WatchArgs {
    /// WebSocket endpoint for `wanlogger serve`.
    #[arg(long, default_value = "ws://127.0.0.1:9000/ws")]
    url: String,
    /// Bearer token. Defaults to `WANLOGGER_TOKEN` when set.
    #[arg(long, env = "WANLOGGER_TOKEN")]
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
    /// Bearer token to hash. Prefer WANLOGGER_TOKEN over passing secrets on a command line.
    #[arg(long, env = "WANLOGGER_TOKEN")]
    token: String,
}

#[derive(Debug, clap::Args)]
struct LogArgs {
    /// Channel spec.
    spec: String,
    /// Output prefix (defaults to `wanlogger`).
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
    /// Importer kind (`teraterm`, `pcapng`, `csv`).
    kind: String,
    /// Source artefact.
    src: std::path::PathBuf,
    /// Destination session-dir.
    dst: std::path::PathBuf,
}

#[derive(Debug, clap::Args)]
struct ExportArgs {
    /// Exporter kind (`csv`, `text`).
    kind: String,
    /// Format exported timestamps in a fixed timezone (`UTC`, `GMT+9`, `+09:00`, `Asia/Tokyo`).
    #[arg(long)]
    tz: Option<String>,
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
        Cmd::Connect(args) => cmd::connect::run(&args.spec).await?,
        Cmd::Send(args) => run_send(args).await?,
        Cmd::Watch(args) => run_watch(args).await?,
        Cmd::TokenHash(args) => run_token_hash(&args)?,
        Cmd::Detect => cmd::detect::run()?,
        Cmd::Log(args) => run_log(args).await?,
        Cmd::Profile(args) => run_profile(args)?,
        Cmd::Replay(args) => {
            wanlogger_replay::run(&args.session, args.rate, args.seed).await?;
        }
        Cmd::Extcap(args) => run_extcap(args).await?,
        Cmd::Import(args) => cmd::import::run(&args.kind, &args.src, &args.dst).await?,
        Cmd::Export(args) => {
            cmd::export::run(&args.kind, &args.src, &args.dst, args.tz.as_deref())?;
        }
        Cmd::AiVerify => ai_verify::run().await?,
        Cmd::JsonSchema(args) => json_schema::emit(&args.out)?,
    }
    Ok(())
}

async fn run_serve(args: ServeArgs) -> anyhow::Result<()> {
    let classifier = cmd::log::classifier_from_specs(&args.classify)?;
    let startup = serve_startup_sources(&args);
    let security = serve_security(&args);
    wanlogger_server::run_with_session_root_classifier_encoding_pattern_startup_and_security(
        &args.bind,
        args.no_auth,
        args.session_root,
        classifier,
        args.encoding,
        args.session_name_pattern.unwrap_or_else(|| {
            wanlogger_core::session_name::DEFAULT_SERVER_SESSION_NAME_PATTERN.to_string()
        }),
        startup,
        security,
    )
    .await
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
    println!("{}", wanlogger_server::auth::hash_token(&args.token)?);
    Ok(())
}

async fn run_log(args: LogArgs) -> anyhow::Result<()> {
    cmd::log::run(cmd::log::Options {
        spec: args.spec,
        prefix: args.prefix,
        name_pattern: args.name_pattern,
        encoding: args.encoding,
        classify: args.classify,
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

fn serve_startup_sources(args: &ServeArgs) -> wanlogger_server::StartupSources {
    if !args.open_all_serial {
        return wanlogger_server::StartupSources::default();
    }
    wanlogger_server::StartupSources {
        serial: Some(wanlogger_server::SerialAutostart {
            ports: if args.serial_ports.is_empty() {
                None
            } else {
                Some(args.serial_ports.clone())
            },
            options: wanlogger_server::source_manager::SerialPortOptions {
                baud: args.serial_baud,
                data_bits: args.serial_data_bits,
                parity: args.serial_parity.clone(),
                stop_bits: args.serial_stop_bits,
                flow: args.serial_flow.clone(),
            },
        }),
    }
}

fn serve_security(args: &ServeArgs) -> wanlogger_server::ServerSecurity {
    let tls = if args.tls || args.tls_dir.is_some() {
        Some(wanlogger_server::TlsServeConfig {
            dir: args
                .tls_dir
                .clone()
                .unwrap_or_else(|| args.session_root.join("tls")),
        })
    } else {
        None
    };
    wanlogger_server::ServerSecurity {
        token_phc: args.token_phc.clone(),
        token_phc_files: args.token_phc_files.clone(),
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
            "wanlogger",
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
        let startup = serve_startup_sources(&args);
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
            "wanlogger",
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
        let security = serve_security(&args);
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
    fn token_hash_args_parse_token() {
        let cli = Cli::try_parse_from(["wanlogger", "token-hash", "--token", "secret"]).unwrap();
        let Cmd::TokenHash(args) = cli.cmd else {
            panic!("expected token-hash command");
        };
        assert_eq!(args.token, "secret");
    }
}
