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
//!
//! [[federation.accept]]
//! object = "zQm..."
//! from = "zQm..."
//! publish = false
//! ```

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use kairo_core::{ActorId, ObjectId};
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
    /// Objects this node accepts pushes for (`DECISIONS.md` §26
    /// sub-decision 2), in file order.
    pub accept: Vec<AcceptConfig>,
    /// How the daemon schedules background sync (`DECISIONS.md` §28).
    pub sync: SyncConfig,
    /// Nodes asked where others are (`DECISIONS.md` §18 sub-decision
    /// 1, §29). Being asked grants nothing else.
    pub bootstrap: Vec<PeerConfig>,
    pub discovery: DiscoveryConfig,
}

/// `[federation.discovery]` (`DECISIONS.md` §29 sub-decision 5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryConfig {
    /// Hints older than this, by the inventory's `signed_at`, are
    /// ignored.
    pub hint_ttl_secs: u64,
    /// Sync followed objects from trusted hinted holders too.
    pub use_hints: bool,
    /// The actor (normally this node's actor) whose `federation:index`
    /// trust vouches for a hinted holder. Required by `use_hints`.
    pub vouched_by: Option<ActorId>,
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        Self {
            hint_ttl_secs: 7 * 24 * 60 * 60,
            use_hints: false,
            vouched_by: None,
        }
    }
}

/// `[federation.sync]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyncConfig {
    /// Sync each follow this often.
    pub interval_secs: u64,
    /// Process the inbox this often.
    pub inbox_secs: u64,
    /// Most follows syncing at once.
    pub max_concurrent: u64,
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            interval_secs: 900,
            inbox_secs: 30,
            max_concurrent: 2,
        }
    }
}

impl Default for FederationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            limits: LimitsConfig::default(),
            peers: Vec::new(),
            accept: Vec::new(),
            sync: SyncConfig::default(),
            bootstrap: Vec::new(),
            discovery: DiscoveryConfig::default(),
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
    /// Largest uploaded bundle a node accepts (`DECISIONS.md` §26
    /// sub-decision 3). See [`DEFAULT_MAX_PUSH_BYTES`].
    pub max_push_bytes: Option<u64>,
    /// Most unprocessed inbox entries per pushing node. See
    /// [`DEFAULT_MAX_INBOX_ENTRIES`].
    pub max_inbox_entries: Option<u64>,
}

/// Default `max_push_bytes`: 1 GiB.
pub const DEFAULT_MAX_PUSH_BYTES: u64 = 1024 * 1024 * 1024;

/// Default `max_inbox_entries`.
pub const DEFAULT_MAX_INBOX_ENTRIES: u64 = 64;

impl LimitsConfig {
    pub fn max_push_bytes(&self) -> u64 {
        self.max_push_bytes.unwrap_or(DEFAULT_MAX_PUSH_BYTES)
    }

    pub fn max_inbox_entries(&self) -> u64 {
        self.max_inbox_entries.unwrap_or(DEFAULT_MAX_INBOX_ENTRIES)
    }
}

/// One `[[federation.accept]]` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptConfig {
    pub object: ObjectId,
    /// The one node actor this entry accepts from; `None` is any node
    /// trusted with `federation:push`.
    pub from: Option<ActorId>,
    /// Serve the object onward after ingest.
    pub publish: bool,
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

    /// The accept entry covering a push of `object` from `from`: one
    /// naming `from` if there is one, else one for any node.
    pub fn accepts(&self, object: &ObjectId, from: &ActorId) -> Option<&AcceptConfig> {
        let for_object = || self.accept.iter().filter(|entry| &entry.object == object);
        for_object()
            .find(|entry| entry.from.as_ref() == Some(from))
            .or_else(|| for_object().find(|entry| entry.from.is_none()))
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
    #[serde(default)]
    accept: Vec<RawAccept>,
    #[serde(default)]
    sync: Option<RawSync>,
    #[serde(default)]
    bootstrap: Vec<RawPeer>,
    #[serde(default)]
    discovery: Option<RawDiscovery>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDiscovery {
    hint_ttl_secs: Option<u64>,
    use_hints: Option<bool>,
    vouched_by: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSync {
    interval_secs: Option<u64>,
    inbox_secs: Option<u64>,
    max_concurrent: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAccept {
    object: String,
    #[serde(default)]
    from: Option<String>,
    #[serde(default)]
    publish: bool,
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
    max_push_bytes: Option<u64>,
    max_inbox_entries: Option<u64>,
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
        ("max_push_bytes", limits.max_push_bytes),
        ("max_inbox_entries", limits.max_inbox_entries),
    ] {
        if value == Some(0) {
            return Err(invalid(format!(
                "federation.limits.{name} must be greater than 0"
            )));
        }
    }

    let peers = parse_peers(federation.peers, "federation.peers", &invalid)?;
    let bootstrap = parse_peers(federation.bootstrap, "federation.bootstrap", &invalid)?;
    let raw_discovery = federation.discovery.unwrap_or_default();
    if raw_discovery.hint_ttl_secs == Some(0) {
        return Err(invalid(
            "federation.discovery.hint_ttl_secs must be greater than 0".to_owned(),
        ));
    }
    let vouched_by = raw_discovery
        .vouched_by
        .as_ref()
        .map(|raw| {
            ActorId::new(raw.clone()).map_err(|error| {
                invalid(format!(
                    "federation.discovery: vouched_by {raw:?} is not a bare actor id: {error}"
                ))
            })
        })
        .transpose()?;
    if raw_discovery.use_hints == Some(true) && vouched_by.is_none() {
        return Err(invalid(
            "federation.discovery: use_hints needs vouched_by, the actor whose federation:index \
             trust vouches for hinted holders"
                .to_owned(),
        ));
    }
    let discovery_defaults = DiscoveryConfig::default();
    let discovery = DiscoveryConfig {
        hint_ttl_secs: raw_discovery
            .hint_ttl_secs
            .unwrap_or(discovery_defaults.hint_ttl_secs),
        use_hints: raw_discovery
            .use_hints
            .unwrap_or(discovery_defaults.use_hints),
        vouched_by,
    };
    let raw_sync = federation.sync.unwrap_or_default();
    for (name, value) in [
        ("interval_secs", raw_sync.interval_secs),
        ("inbox_secs", raw_sync.inbox_secs),
        ("max_concurrent", raw_sync.max_concurrent),
    ] {
        if value == Some(0) {
            return Err(invalid(format!(
                "federation.sync.{name} must be greater than 0"
            )));
        }
    }
    let defaults = SyncConfig::default();
    let sync = SyncConfig {
        interval_secs: raw_sync.interval_secs.unwrap_or(defaults.interval_secs),
        inbox_secs: raw_sync.inbox_secs.unwrap_or(defaults.inbox_secs),
        max_concurrent: raw_sync.max_concurrent.unwrap_or(defaults.max_concurrent),
    };

    let mut accept: Vec<AcceptConfig> = Vec::with_capacity(federation.accept.len());
    for raw in federation.accept {
        let object = ObjectId::new(raw.object.clone()).map_err(|error| {
            invalid(format!(
                "federation.accept: object {:?} is not a bare object id: {error}",
                raw.object
            ))
        })?;
        let from = raw
            .from
            .map(|from| {
                ActorId::new(from.clone()).map_err(|error| {
                    invalid(format!(
                        "federation.accept: from {from:?} for {object} is not a bare actor id: \
                         {error}"
                    ))
                })
            })
            .transpose()?;
        if accept
            .iter()
            .any(|entry| entry.object == object && entry.from == from)
        {
            return Err(invalid(format!(
                "federation.accept: {object} is listed twice for the same from"
            )));
        }
        accept.push(AcceptConfig {
            object,
            from,
            publish: raw.publish,
        });
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
                max_push_bytes: limits.max_push_bytes,
                max_inbox_entries: limits.max_inbox_entries,
            },
            peers,
            accept,
            sync,
            bootstrap,
            discovery,
        },
    })
}

/// `[[federation.peers]]` or `[[federation.bootstrap]]` entries: a
/// non-empty URL (trailing `/` dropped), listed once, and an optional
/// bare-id `node_actor` pin.
fn parse_peers(
    raw: Vec<RawPeer>,
    table: &str,
    invalid: &impl Fn(String) -> ConfigError,
) -> Result<Vec<PeerConfig>, ConfigError> {
    let mut peers: Vec<PeerConfig> = Vec::with_capacity(raw.len());
    for raw_peer in raw {
        let url = raw_peer
            .url
            .strip_suffix('/')
            .unwrap_or(&raw_peer.url)
            .to_owned();
        if url.is_empty() {
            return Err(invalid(format!("{table}: url is empty")));
        }
        if peers.iter().any(|peer| peer.url == url) {
            return Err(invalid(format!("{table}: {url} is listed twice")));
        }
        let node_actor = raw_peer
            .node_actor
            .map(|raw| {
                ActorId::new(raw.clone()).map_err(|error| {
                    invalid(format!(
                        "{table}: node_actor {raw:?} for {url} is not a bare actor id: {error}"
                    ))
                })
            })
            .transpose()?;
        peers.push(PeerConfig { url, node_actor });
    }
    Ok(peers)
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

    const OBJECT: &str = "zQmZbDPpjCCRPGVC13GXNG7S3o6FtefW6EtdHcACKMNquAS";
    const OTHER_ACTOR: &str = "zQmb7GGJWDbEMnz5VM51ijuLtzguKf1qhxzqPh8xZBkmBW9";

    #[test]
    fn parses_the_accept_list_and_push_limits() {
        let config = parse_ok(&format!(
            r#"
            [federation.limits]
            max_push_bytes = 4096
            max_inbox_entries = 2

            [[federation.accept]]
            object = "{OBJECT}"

            [[federation.accept]]
            object = "{OBJECT}"
            from = "{ACTOR}"
            publish = true
            "#
        ));
        let federation = config.federation;
        assert_eq!(federation.limits.max_push_bytes(), 4096);
        assert_eq!(federation.limits.max_inbox_entries(), 2);
        assert_eq!(
            LimitsConfig::default().max_push_bytes(),
            DEFAULT_MAX_PUSH_BYTES
        );
        let object = ObjectId::new(OBJECT).unwrap_or_else(|e| panic!("{e}"));
        let actor = ActorId::new(ACTOR).unwrap_or_else(|e| panic!("{e}"));
        let other = ActorId::new(OTHER_ACTOR).unwrap_or_else(|e| panic!("{e}"));
        // The entry naming the pusher wins over the wildcard.
        assert_eq!(
            federation.accepts(&object, &actor).map(|e| e.publish),
            Some(true)
        );
        assert_eq!(
            federation.accepts(&object, &other).map(|e| e.publish),
            Some(false)
        );
        let elsewhere = ObjectId::from_sha256_digest([9; 32]);
        assert!(federation.accepts(&elsewhere, &actor).is_none());
    }

    #[test]
    fn parses_sync_scheduling_with_defaults() {
        assert_eq!(Config::default().federation.sync, SyncConfig::default());
        let config = parse_ok("[federation.sync]\ninterval_secs = 60\n");
        assert_eq!(config.federation.sync.interval_secs, 60);
        assert_eq!(config.federation.sync.inbox_secs, 30);
        assert!(parse_err("[federation.sync]\ninbox_secs = 0\n").contains("inbox_secs"));
        assert!(parse_err("[federation.sync]\nevery = 5\n").contains("every"));
    }

    #[test]
    fn parses_bootstrap_nodes_and_discovery() {
        let config = parse_ok(&format!(
            "[[federation.bootstrap]]\nurl = \"https://seed.example.org/\"\nnode_actor = \"{ACTOR}\"\n\n\
             [federation.discovery]\nuse_hints = true\nvouched_by = \"{ACTOR}\"\n"
        ));
        let federation = config.federation;
        assert_eq!(federation.bootstrap.len(), 1);
        assert_eq!(federation.bootstrap[0].url, "https://seed.example.org");
        assert!(federation.peers.is_empty(), "bootstrap nodes are not peers");
        assert!(federation.discovery.use_hints);
        assert_eq!(
            federation.discovery.hint_ttl_secs,
            DiscoveryConfig::default().hint_ttl_secs
        );
        assert!(!DiscoveryConfig::default().use_hints);
        assert!(parse_err("[federation.discovery]\nhint_ttl_secs = 0\n").contains("hint_ttl_secs"));
        assert!(parse_err("[federation.discovery]\nuse_hints = true\n").contains("vouched_by"));
        assert!(parse_err(
            "[[federation.bootstrap]]\nurl = \"https://a.example\"\n\n[[federation.bootstrap]]\nurl = \"https://a.example/\"\n"
        )
        .contains("federation.bootstrap"));
    }

    #[test]
    fn invalid_accept_entries_are_errors() {
        assert!(parse_err(
            "[[federation.accept]]
object = \"object:x\"
"
        )
        .contains("bare object id"));
        assert!(parse_err(&format!(
            "[[federation.accept]]
object = \"{OBJECT}\"
from = \"kairo:actor:x\"
"
        ))
        .contains("bare actor id"));
        assert!(parse_err(&format!(
            "[[federation.accept]]
object = \"{OBJECT}\"

[[federation.accept]]
object = \"{OBJECT}\"
"
        ))
        .contains("listed twice"));
        assert!(parse_err(&format!(
            "[[federation.accept]]
object = \"{OBJECT}\"
mirror = true
"
        ))
        .contains("mirror"));
        assert!(parse_err(
            "[federation.limits]
max_inbox_entries = 0
"
        )
        .contains("max_inbox_entries"));
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
