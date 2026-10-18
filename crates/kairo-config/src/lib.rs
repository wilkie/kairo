//! `kairo-config` — the node configuration file (`specs/DECISIONS.md`
//! §17).
//!
//! A node reads `<store>/config.toml` (so `~/.kairo/config.toml` for
//! the default store). A missing file means every default applies.
//! Parsing is strict: an unknown section or key is an error rather
//! than silently ignored, so a typo in a peer pin can't quietly turn
//! pinning off. Only the keys implemented so far are accepted; the
//! other sections `DAEMON.md` §15 sketches (`[api]`, `[policy]`,
//! `[runtime]`) are added as their features land.
//!
//! ```toml
//! [federation]
//! enabled = true
//!
//! [federation.limits]
//! max_blob_bytes = 268435456
//!
//! [[federation.peers]]
//! url = "https://archive.example.org"
//! node_actor = "zQm..."
//! ```

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use kairo_core::ActorId;
use serde::Deserialize;

/// File name inside the store root.
pub const CONFIG_FILE: &str = "config.toml";

/// The config file's path for a store root.
pub fn config_path(store_root: &Path) -> PathBuf {
    store_root.join(CONFIG_FILE)
}

/// A parsed, validated configuration.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Config {
    pub federation: FederationConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FederationConfig {
    /// `false` puts the node in local-only mode (`DAEMON.md` §10.1):
    /// `kairo fetch` refuses to run. Defaults to `true`.
    pub enabled: bool,
    pub limits: LimitsConfig,
    /// Peers this node may pull from, in file order.
    pub peers: Vec<PeerConfig>,
}

impl Default for FederationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            limits: LimitsConfig::default(),
            peers: Vec::new(),
        }
    }
}

/// Overrides for the pull client's limits; `None` keeps its default.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LimitsConfig {
    pub max_json_bytes: Option<u64>,
    /// Largest statement batch response (`specs/DECISIONS.md` §24).
    pub max_batch_bytes: Option<u64>,
    pub max_blob_bytes: Option<u64>,
    pub max_git_bytes: Option<u64>,
    pub max_records: Option<u64>,
    pub request_timeout_secs: Option<u64>,
    pub git_timeout_secs: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerConfig {
    /// As written, minus a trailing `/`. Callers compare it after
    /// their own URL normalization.
    pub url: String,
    /// The node actor this peer must claim. `None` accepts whatever
    /// it claims (the pull reports the claim so it can be pinned).
    pub node_actor: Option<ActorId>,
}

impl FederationConfig {
    /// The entry for `url` (compared without a trailing `/`).
    pub fn peer(&self, url: &str) -> Option<&PeerConfig> {
        let url = url.strip_suffix('/').unwrap_or(url);
        self.peers.iter().find(|peer| peer.url == url)
    }
}

// --- raw file shape ----------------------------------------------------

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    #[serde(default)]
    federation: Option<RawFederation>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFederation {
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    limits: Option<RawLimits>,
    #[serde(default)]
    peers: Vec<RawPeer>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLimits {
    max_json_bytes: Option<u64>,
    max_batch_bytes: Option<u64>,
    max_blob_bytes: Option<u64>,
    max_git_bytes: Option<u64>,
    max_records: Option<u64>,
    request_timeout_secs: Option<u64>,
    git_timeout_secs: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPeer {
    url: String,
    #[serde(default)]
    node_actor: Option<String>,
}

// --- loading -----------------------------------------------------------

#[derive(Debug)]
pub enum ConfigError {
    Read { path: PathBuf, source: io::Error },
    Parse { path: PathBuf, message: String },
    Invalid { path: PathBuf, message: String },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, source } => write!(f, "cannot read {}: {source}", path.display()),
            Self::Parse { path, message } | Self::Invalid { path, message } => {
                write!(f, "{}: {message}", path.display())
            }
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read { source, .. } => Some(source),
            Self::Parse { .. } | Self::Invalid { .. } => None,
        }
    }
}

/// Where a loaded configuration came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigSource {
    File(PathBuf),
    /// No file at this path; defaults apply.
    Defaults(PathBuf),
}

impl ConfigSource {
    pub fn path(&self) -> &Path {
        match self {
            Self::File(path) | Self::Defaults(path) => path,
        }
    }
}

/// Load `<store_root>/config.toml`, or the defaults if it's absent.
pub fn load(store_root: &Path) -> Result<(Config, ConfigSource), ConfigError> {
    let path = config_path(store_root);
    match std::fs::read_to_string(&path) {
        Ok(text) => Ok((parse(&text, &path)?, ConfigSource::File(path))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            Ok((Config::default(), ConfigSource::Defaults(path)))
        }
        Err(source) => Err(ConfigError::Read { path, source }),
    }
}

/// Parse and validate config text. `path` only labels errors.
pub fn parse(text: &str, path: &Path) -> Result<Config, ConfigError> {
    let raw: RawConfig = toml::from_str(text).map_err(|error| ConfigError::Parse {
        path: path.to_path_buf(),
        message: error.message().to_owned(),
    })?;
    let invalid = |message: String| ConfigError::Invalid {
        path: path.to_path_buf(),
        message,
    };

    let Some(federation) = raw.federation else {
        return Ok(Config::default());
    };
    let limits = federation.limits.unwrap_or_default();
    for (name, value) in [
        ("max_json_bytes", limits.max_json_bytes),
        ("max_batch_bytes", limits.max_batch_bytes),
        ("max_blob_bytes", limits.max_blob_bytes),
        ("max_git_bytes", limits.max_git_bytes),
        ("max_records", limits.max_records),
        ("request_timeout_secs", limits.request_timeout_secs),
        ("git_timeout_secs", limits.git_timeout_secs),
    ] {
        if value == Some(0) {
            return Err(invalid(format!(
                "federation.limits.{name} must be greater than 0"
            )));
        }
    }

    let mut peers: Vec<PeerConfig> = Vec::with_capacity(federation.peers.len());
    for raw_peer in federation.peers {
        let url = raw_peer
            .url
            .strip_suffix('/')
            .unwrap_or(&raw_peer.url)
            .to_owned();
        if url.is_empty() {
            return Err(invalid("federation.peers: url is empty".to_owned()));
        }
        if peers.iter().any(|peer| peer.url == url) {
            return Err(invalid(format!("federation.peers: {url} is listed twice")));
        }
        let node_actor = raw_peer
            .node_actor
            .map(|raw| {
                ActorId::new(raw.clone()).map_err(|error| {
                    invalid(format!(
                        "federation.peers: node_actor {raw:?} for {url} is not a bare actor id: \
                         {error}"
                    ))
                })
            })
            .transpose()?;
        peers.push(PeerConfig { url, node_actor });
    }

    Ok(Config {
        federation: FederationConfig {
            enabled: federation.enabled.unwrap_or(true),
            limits: LimitsConfig {
                max_json_bytes: limits.max_json_bytes,
                max_batch_bytes: limits.max_batch_bytes,
                max_blob_bytes: limits.max_blob_bytes,
                max_git_bytes: limits.max_git_bytes,
                max_records: limits.max_records,
                request_timeout_secs: limits.request_timeout_secs,
                git_timeout_secs: limits.git_timeout_secs,
            },
            peers,
        },
    })
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;

    fn parse_ok(text: &str) -> Config {
        match parse(text, Path::new("config.toml")) {
            Ok(config) => config,
            Err(error) => panic!("{error}"),
        }
    }

    fn parse_err(text: &str) -> String {
        match parse(text, Path::new("config.toml")) {
            Ok(config) => panic!("accepted: {config:?}"),
            Err(error) => error.to_string(),
        }
    }

    const ACTOR: &str = "zQmTn1mdQDA1ryQZsiqYfRbqj5DGcG8TNvYcRmBrBLAuk5t";

    #[test]
    fn empty_and_missing_sections_are_defaults() {
        assert_eq!(parse_ok(""), Config::default());
        assert_eq!(parse_ok("[federation]\n"), Config::default());
        assert!(Config::default().federation.enabled);
    }

    #[test]
    fn parses_peers_limits_and_enablement() {
        let config = parse_ok(&format!(
            r#"
            [federation]
            enabled = false

            [federation.limits]
            max_blob_bytes = 1024
            git_timeout_secs = 60

            [[federation.peers]]
            url = "https://archive.example.org/"
            node_actor = "{ACTOR}"

            [[federation.peers]]
            url = "http://127.0.0.1:7880"
            "#
        ));
        let federation = config.federation;
        assert!(!federation.enabled);
        assert_eq!(federation.limits.max_blob_bytes, Some(1024));
        assert_eq!(federation.limits.git_timeout_secs, Some(60));
        assert_eq!(federation.limits.max_json_bytes, None);
        assert_eq!(federation.peers.len(), 2);
        let archive = federation
            .peer("https://archive.example.org")
            .map(|peer| peer.node_actor.as_ref().map(ActorId::as_str));
        assert_eq!(archive, Some(Some(ACTOR)));
        assert!(federation.peer("http://127.0.0.1:7880/").is_some());
        assert!(federation.peer("https://elsewhere.example").is_none());
    }

    #[test]
    fn unknown_keys_and_sections_are_errors() {
        assert!(parse_err("[policy]\ndefault_network_access = \"deny\"\n").contains("policy"));
        assert!(parse_err("[federation]\nenable = true\n").contains("enable"));
        assert!(
            parse_err("[[federation.peers]]\nurl = \"https://a.example\"\nnode = \"x\"\n")
                .contains("node")
        );
    }

    #[test]
    fn invalid_values_are_errors() {
        assert!(parse_err("[federation.limits]\nmax_records = 0\n").contains("max_records"));
        assert!(parse_err(
            "[[federation.peers]]\nurl = \"https://a.example\"\nnode_actor = \"kairo:actor:x\"\n"
        )
        .contains("bare actor id"));
        assert!(parse_err(
            "[[federation.peers]]\nurl = \"https://a.example\"\n\n[[federation.peers]]\nurl = \"https://a.example/\"\n"
        )
        .contains("listed twice"));
        assert!(parse_err("[federation]\nenabled = \"yes\"\n").contains("config.toml"));
    }

    #[test]
    fn load_falls_back_to_defaults_when_the_file_is_absent(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let dir = std::env::temp_dir().join(format!("kairo-config-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let (config, source) = load(&dir)?;
        assert_eq!(config, Config::default());
        assert_eq!(source, ConfigSource::Defaults(dir.join(CONFIG_FILE)));

        std::fs::write(dir.join(CONFIG_FILE), "[federation]\nenabled = false\n")?;
        let (config, source) = load(&dir)?;
        assert!(!config.federation.enabled);
        assert_eq!(source, ConfigSource::File(dir.join(CONFIG_FILE)));
        std::fs::remove_dir_all(&dir)?;
        Ok(())
    }
}
