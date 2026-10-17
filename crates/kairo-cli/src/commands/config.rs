//! `kairo config ...` runners (`specs/CLI.md` §27,
//! `specs/DECISIONS.md` §17): where the node config file is and
//! whether it parses. Editing stays manual.

use kairo_config::ConfigSource;

use crate::cli::ConfigCommand;
use crate::error::CliError;
use crate::store_paths::StorePaths;

pub(crate) fn run_config_command(
    command: ConfigCommand,
    paths: &StorePaths,
) -> Result<String, CliError> {
    match command {
        ConfigCommand::Path => {
            let path = kairo_config::config_path(&paths.store);
            let exists = if path.exists() {
                "yes"
            } else {
                "no (defaults apply)"
            };
            Ok(format!("path = {}\nexists = {exists}\n", path.display()))
        }
        ConfigCommand::Validate => {
            let (config, source) = load_config(paths)?;
            for peer in &config.federation.peers {
                if let Err(error) = kairo_federation_http::client::normalize_peer_url(&peer.url) {
                    return Err(CliError::LoadConfig(kairo_config::ConfigError::Invalid {
                        path: source.path().to_path_buf(),
                        message: format!("federation.peers: {error}"),
                    }));
                }
            }
            let mut output = match &source {
                ConfigSource::File(path) => format!("config = {} (valid)\n", path.display()),
                ConfigSource::Defaults(path) => {
                    format!("config = {} (absent; defaults apply)\n", path.display())
                }
            };
            output.push_str(&format!(
                "federation = {}\nfederation_peers = {}\n",
                if config.federation.enabled {
                    "enabled"
                } else {
                    "disabled"
                },
                config.federation.peers.len()
            ));
            Ok(output)
        }
    }
}

/// Load `<store>/config.toml` (or defaults) for any command that
/// consults it.
pub(crate) fn load_config(
    paths: &StorePaths,
) -> Result<(kairo_config::Config, ConfigSource), CliError> {
    kairo_config::load(&paths.store).map_err(CliError::LoadConfig)
}
