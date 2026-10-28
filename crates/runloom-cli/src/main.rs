use clap::{Parser, Subcommand};
use runloom_core::{NewWork, Store};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "runloom",
    version,
    about = "Runloom — AI work continuity (local metadata preview)"
)]
struct Cli {
    /// Explicit database path. Parent directory must already exist.
    #[arg(long)]
    db: PathBuf,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    Init,
    /// Register metadata only; does not launch or resume an agent.
    Register {
        #[arg(long)]
        workspace: String,
        #[arg(long)]
        host: String,
        #[arg(long)]
        provider: String,
        #[arg(long)]
        config_root: String,
        #[arg(long)]
        native_id: String,
        #[arg(long)]
        directory: String,
        #[arg(long)]
        title: String,
    },
    List {
        #[arg(long)]
        workspace: String,
    },
    Events {
        #[arg(long)]
        workspace: String,
        #[arg(long)]
        work: String,
        #[arg(long, default_value_t = 0)]
        after: i64,
        #[arg(long, default_value_t = 100)]
        limit: u32,
    },
    /// Append an observation, never an executable agent command.
    Note {
        #[arg(long)]
        workspace: String,
        #[arg(long)]
        work: String,
        #[arg(long)]
        request_id: String,
        #[arg(long)]
        text: String,
    },
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let mut store = Store::open(&cli.db)?;
    let output = match cli.command {
        Command::Init => {
            serde_json::json!({"schema":runloom_core::SCHEMA_VERSION,"status":"ready"})
        }
        Command::Register {
            workspace,
            host,
            provider,
            config_root,
            native_id,
            directory,
            title,
        } => serde_json::to_value(store.create_work(NewWork {
            workspace,
            host,
            provider,
            config_root,
            native_id,
            directory,
            title,
        })?)?,
        Command::List { workspace } => serde_json::to_value(store.list(&workspace)?)?,
        Command::Events {
            workspace,
            work,
            after,
            limit,
        } => serde_json::to_value(store.events(&workspace, &work, after, limit)?)?,
        Command::Note {
            workspace,
            work,
            request_id,
            text,
        } => serde_json::to_value(store.append_event(
            &workspace,
            &work,
            &request_id,
            "user.note",
            serde_json::json!({"text":text}),
        )?)?,
    };
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("runloom: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_schema_and_required_database_are_valid() {
        Cli::command().debug_assert();
        assert!(Cli::try_parse_from(["runloom", "init"]).is_err());
        assert!(Cli::try_parse_from(["runloom", "--db", "/tmp/fixture.db", "init"]).is_ok());
    }
}
