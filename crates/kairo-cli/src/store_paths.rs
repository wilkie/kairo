//! `StorePaths` resolves the on-disk locations the CLI reads and writes
//! before any command runs. `--store` and `--keys` override the defaults
//! (`$HOME/.kairo` and `<store>/keys` respectively); commands then take a
//! borrowed `&StorePaths` rather than re-parsing the args themselves.

use std::path::PathBuf;

use kairo_keystore::FilesystemKeystore;
use kairo_store::FilesystemStore;

use crate::error::CliError;

#[derive(Debug, Clone)]
pub(crate) struct StorePaths {
    pub(crate) store: PathBuf,
    pub(crate) keys: PathBuf,
}

impl StorePaths {
    pub(crate) fn resolve(
        store: Option<PathBuf>,
        keys: Option<PathBuf>,
    ) -> Result<Self, CliError> {
        let store = match store {
            Some(path) => path,
            None => default_store_root()?,
        };
        let keys = keys.unwrap_or_else(|| store.join("keys"));
        Ok(Self { store, keys })
    }
}

fn default_store_root() -> Result<PathBuf, CliError> {
    match std::env::var_os("HOME") {
        Some(home) => Ok(PathBuf::from(home).join(".kairo")),
        None => Err(CliError::HomeNotSet),
    }
}

pub(crate) fn open_store(paths: &StorePaths) -> Result<FilesystemStore, CliError> {
    FilesystemStore::open(&paths.store).map_err(|error| CliError::OpenStore {
        path: paths.store.clone(),
        source: error,
    })
}

pub(crate) fn open_keystore(paths: &StorePaths) -> Result<FilesystemKeystore, CliError> {
    FilesystemKeystore::open(&paths.keys).map_err(|error| CliError::OpenKeystore {
        path: paths.keys.clone(),
        source: error,
    })
}
