//! `kairo-federation-http` binary entry point.
//!
//! Foreground only, like `kairo-web`. Parses flags, installs the
//! tracing subscriber, and runs [`serve`] until `SIGTERM` / `SIGINT`.

use std::env;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use kairo_core::ActorId;
use kairo_federation_http::{install_tracing, serve, Config, DEFAULT_BIND, DEFAULT_VIEW_TTL};

// The bin only drives the lib's public API; these shims silence
// `unused_crate_dependencies` for crates it doesn't name.
use axum as _;
use http_body as _;
#[cfg(test)]
use http_body_util as _;
use kairo_federation_core as _;
use kairo_git as _;
use kairo_identity as _;
#[cfg(feature = "client")]
use kairo_object as _;
use kairo_statement as _;
use kairo_store as _;
#[cfg(test)]
use kairo_test_support as _;
use serde as _;
use serde_json as _;
#[cfg(test)]
use tempfile as _;
#[cfg(test)]
use tower as _;
use tracing as _;
use tracing_subscriber as _;
#[cfg(feature = "client")]
use ureq as _;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const USAGE: &str = "\
usage: kairo-federation-http --store <path> --node-actor <id>
                             [--bind <addr>] [--base-url <url>] [--view-ttl <secs>]
                             [--max-concurrent-packs <n>]
       kairo-federation-http --version

Serves this node's published objects to federation peers over plain
HTTP (DECISIONS.md §16). Binds 127.0.0.1:7880 by default; put a
TLS-terminating reverse proxy in front and pass its public origin as
--base-url. --base-url defaults to http://<bind>.
";

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.iter().any(|arg| arg == "--version" || arg == "-V") {
        println!("kairo-federation-http {VERSION}");
        return ExitCode::SUCCESS;
    }
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }

    let config = match parse_args(&args) {
        Ok(config) => config,
        Err(message) => {
            eprintln!("kairo-federation-http: {message}");
            eprint!("{USAGE}");
            return ExitCode::from(2);
        }
    };

    install_tracing();

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("kairo-federation-http: failed to start async runtime: {error}");
            return ExitCode::from(1);
        }
    };
    match runtime.block_on(serve(config)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("kairo-federation-http: {error}");
            ExitCode::from(1)
        }
    }
}

fn parse_args(args: &[String]) -> Result<Config, String> {
    let mut iter = args.iter().skip(1);
    let mut store: Option<PathBuf> = None;
    let mut node_actor: Option<ActorId> = None;
    let mut bind_addr: SocketAddr = DEFAULT_BIND;
    let mut base_url: Option<String> = None;
    let mut view_ttl = DEFAULT_VIEW_TTL;
    let mut max_concurrent_packs: Option<usize> = None;

    while let Some(arg) = iter.next() {
        let mut value = |flag: &str| {
            iter.next()
                .cloned()
                .ok_or_else(|| format!("{flag} requires a value"))
        };
        match arg.as_str() {
            "--store" => store = Some(PathBuf::from(value("--store")?)),
            "--node-actor" => {
                let raw = value("--node-actor")?;
                node_actor = Some(
                    ActorId::new(raw.clone())
                        .map_err(|error| format!("--node-actor {raw:?}: {error}"))?,
                );
            }
            "--bind" => {
                let raw = value("--bind")?;
                bind_addr = raw
                    .parse()
                    .map_err(|error| format!("--bind {raw:?}: {error}"))?;
            }
            "--base-url" => base_url = Some(value("--base-url")?),
            "--view-ttl" => {
                let raw = value("--view-ttl")?;
                let secs: u64 = raw
                    .parse()
                    .map_err(|error| format!("--view-ttl {raw:?}: {error}"))?;
                view_ttl = Duration::from_secs(secs);
            }
            "--max-concurrent-packs" => {
                let raw = value("--max-concurrent-packs")?;
                let max: usize = raw.parse().ok().filter(|n| *n > 0).ok_or_else(|| {
                    format!("--max-concurrent-packs {raw:?}: expected a positive integer")
                })?;
                max_concurrent_packs = Some(max);
            }
            other => return Err(format!("unknown argument {other:?}")),
        }
    }

    Ok(Config {
        store: store.ok_or("--store is required")?,
        node_actor: node_actor.ok_or("--node-actor is required")?,
        base_url: base_url.unwrap_or_else(|| format!("http://{bind_addr}")),
        bind_addr,
        view_ttl,
        max_concurrent_packs,
    })
}
