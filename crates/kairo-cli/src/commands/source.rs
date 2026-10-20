//! `kairo source ...` runners (`specs/DECISIONS.md` §37): source
//! objects, external Git repositories identified by their canonical URL.

use kairo_statement::{SourceGenesis, SourceUrl};

use crate::cli::SourceCommand;
use crate::error::CliError;

pub(crate) fn run_source_command(command: SourceCommand) -> Result<String, CliError> {
    match command {
        SourceCommand::Id { url } => {
            let url = SourceUrl::parse(&url).map_err(CliError::ParseSourceUrl)?;
            let source = SourceGenesis::new(url.clone()).object_id();
            Ok(format!("url = {url}\nsource = {source}\n"))
        }
    }
}
