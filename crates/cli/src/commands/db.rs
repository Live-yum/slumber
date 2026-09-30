//! All operations related to direct DB modification live in here. Since these
//! are fairly niche and advanced, we group them all together to not pollute
//! the global command namespace with useless stuff.

mod collection;
mod request;
mod shell;

use crate::{
    GlobalArgs, Subcommand,
    commands::db::{
        collection::DbCollectionCommand, request::DbRequestCommand,
    },
};
use anyhow::Context;
use clap::Parser;
use slumber_core::database::Database;
use std::process::ExitCode;
use tokio::process::Command;

/// Access and modify the Slumber database (collection and request history)
///
/// Without a subcommand this opens the built-in SQLite console.
/// Pass SQL as arguments or via stdin. No sqlite3 installation is required.
/// Supports .tables, .schema, .help, .quit and SQLite SQL; results are JSON.
/// --exec explicitly selects an external client in normal mode only.
#[derive(Clone, Debug, Parser)]
#[clap(verbatim_doc_comment)]
pub struct DbCommand {
    #[command(subcommand)]
    subcommand: Option<DbSubcommand>,
    /// Program to execute
    #[clap(short = 'x', long)]
    exec: Option<String>,
    /// SQL statements for the built-in console, or arguments to an explicitly
    /// selected external --exec program. SQL can be passed like so:
    ///
    ///   slumber db 'select 1'
    ///
    /// However if you want to pass flags that begin with "-", you have to
    /// precede the forwarded arguments with "--" to separate them from
    /// arguments intended for `slumber`.
    ///
    ///   slumber db -- -cmd 'select 1'
    #[clap(num_args = 1.., verbatim_doc_comment)]
    args: Vec<String>,
    /// Print the path of the database file and exit; overrides all other
    /// arguments
    #[clap(long)]
    path: bool,
}

#[derive(Clone, Debug, clap::Subcommand)]
enum DbSubcommand {
    #[command(visible_alias = "coll")]
    Collection(DbCollectionCommand),
    #[command(visible_alias = "rq")]
    Request(DbRequestCommand),
}

impl Subcommand for DbCommand {
    async fn execute(self, global: GlobalArgs) -> anyhow::Result<ExitCode> {
        match self.subcommand {
            None => {
                let path = Database::path();

                if self.path {
                    println!("{}", path.display());
                    return Ok(ExitCode::SUCCESS);
                }

                let Some(executable) = self.exec else {
                    shell::run(&self.args)?;
                    return Ok(ExitCode::SUCCESS);
                };
                if slumber_util::paths::portable_directory().is_some() {
                    anyhow::bail!(
                        "db --exec is disabled in portable mode; use the built-in SQL console"
                    );
                }
                let exit_status = Command::new(executable)
                    .arg(&path)
                    .args(self.args)
                    .spawn()
                    .with_context(|| {
                        format!(
                            "Error opening database file `{}`",
                            path.display()
                        )
                    })?
                    .wait()
                    .await?;

                // Forward exit code if we can, otherwise just do success/fail
                let exit_code =
                    exit_status.code().and_then(|code| u8::try_from(code).ok());
                if let Some(code) = exit_code {
                    Ok(code.into())
                } else if exit_status.success() {
                    Ok(ExitCode::SUCCESS)
                } else {
                    Ok(ExitCode::FAILURE)
                }
            }
            Some(DbSubcommand::Collection(command)) => {
                command.execute(global).await
            }
            Some(DbSubcommand::Request(command)) => {
                command.execute(global).await
            }
        }
    }
}
