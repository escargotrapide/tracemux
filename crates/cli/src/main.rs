//! `wanlogger` — single binary CLI / server.
//!
//! Subcommands: `serve | connect | detect | log | profile | replay |
//! extcap | import | export | ai-verify | json-schema`.

use clap::{Parser, Subcommand};

mod ai_verify;
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
    Profile,
    /// Replay an existing session-dir.
    Replay(ReplayArgs),
    /// Wireshark extcap interface — stub.
    Extcap,
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
            wanlogger_server::run(&args.bind, args.no_auth).await?;
        }
        Cmd::Connect(args) => {
            tracing::warn!(spec = %args.spec, "connect: v0.1 stub");
        }
        Cmd::Detect => tracing::warn!("detect: v0.1 stub"),
        Cmd::Log(args) => tracing::warn!(spec = %args.spec, "log: v0.1 stub"),
        Cmd::Profile => tracing::warn!("profile: v0.1 stub"),
        Cmd::Replay(args) => {
            wanlogger_replay::run(&args.session, args.rate, args.seed).await?;
        }
        Cmd::Extcap => tracing::warn!("extcap: v0.1 stub"),
        Cmd::Import(args) => {
            tracing::warn!(kind = %args.kind, src = ?args.src, dst = ?args.dst, "import: v0.1 stub");
        }
        Cmd::Export(args) => {
            tracing::warn!(kind = %args.kind, src = ?args.src, dst = ?args.dst, "export: v0.1 stub");
        }
        Cmd::AiVerify => ai_verify::run().await?,
        Cmd::JsonSchema(args) => json_schema::emit(&args.out)?,
    }
    Ok(())
}
