//! `wanlogger` — single binary CLI / server.
//!
//! Subcommands: `serve | connect | detect | log | profile | replay |
//! extcap | import | export | ai-verify | json-schema`.

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
    /// Disable auth (gated to loopback).
    #[arg(long)]
    no_auth: bool,
}

#[derive(Debug, clap::Args)]
struct ConnectArgs {
    /// Channel spec, e.g. `serial://COM3?baud=115200`.
    spec: String,
}

#[derive(Debug, clap::Args)]
struct LogArgs {
    /// Channel spec.
    spec: String,
    /// Output prefix (defaults to `wanlogger`).
    #[arg(long)]
    prefix: Option<String>,
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
    match cli.cmd {
        Cmd::Serve(args) => {
            wanlogger_server::run_with_session_root(&args.bind, args.no_auth, args.session_root)
                .await?;
        }
        Cmd::Connect(args) => cmd::connect::run(&args.spec).await?,
        Cmd::Detect => cmd::detect::run()?,
        Cmd::Log(args) => cmd::log::run(&args.spec, args.prefix.as_deref()).await?,
        Cmd::Profile(args) => {
            let dir = args.dir.unwrap_or_else(cmd::profile::default_dir);
            let action = match args.action {
                ProfileAction::List => cmd::profile::Action::List,
                ProfileAction::Show { name } => cmd::profile::Action::Show { name },
                ProfileAction::Set { name, spec } => cmd::profile::Action::Set { name, spec },
                ProfileAction::Del { name } => cmd::profile::Action::Del { name },
            };
            cmd::profile::run(&dir, action)?;
        }
        Cmd::Replay(args) => {
            wanlogger_replay::run(&args.session, args.rate, args.seed).await?;
        }
        Cmd::Extcap(args) => {
            let mode = if args.extcap_interfaces {
                cmd::extcap::Mode::Interfaces
            } else if args.extcap_dlts {
                cmd::extcap::Mode::Dlts {
                    interface: args.extcap_interface.ok_or_else(|| {
                        anyhow::anyhow!("--extcap-dlts requires --extcap-interface")
                    })?,
                }
            } else if args.extcap_config {
                cmd::extcap::Mode::Config {
                    interface: args.extcap_interface.ok_or_else(|| {
                        anyhow::anyhow!("--extcap-config requires --extcap-interface")
                    })?,
                }
            } else if args.capture {
                cmd::extcap::Mode::Capture {
                    interface: args
                        .extcap_interface
                        .ok_or_else(|| anyhow::anyhow!("--capture requires --extcap-interface"))?,
                    fifo: args
                        .fifo
                        .ok_or_else(|| anyhow::anyhow!("--capture requires --fifo"))?,
                    spec: args
                        .spec
                        .ok_or_else(|| anyhow::anyhow!("--capture requires --spec"))?,
                }
            } else {
                anyhow::bail!("extcap: one of --extcap-interfaces / --extcap-dlts / --extcap-config / --capture is required");
            };
            cmd::extcap::run(mode).await?;
        }
        Cmd::Import(args) => cmd::import::run(&args.kind, &args.src, &args.dst).await?,
        Cmd::Export(args) => cmd::export::run(&args.kind, &args.src, &args.dst).await?,
        Cmd::AiVerify => ai_verify::run().await?,
        Cmd::JsonSchema(args) => json_schema::emit(&args.out)?,
    }
    Ok(())
}
