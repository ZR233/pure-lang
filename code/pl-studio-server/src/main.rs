use std::net::SocketAddr;
use std::path::PathBuf;

use clap::{Parser, Subcommand};
use pl_studio_server::{DEFAULT_LISTEN, ServerOptions};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(
    name = "pl-studio-server",
    version,
    about = "anywork loopback HTTP API"
)]
struct Cli {
    #[arg(long, default_value = DEFAULT_LISTEN)]
    listen: SocketAddr,
    #[arg(long, value_name = "ABSOLUTE_PATH")]
    studio_home: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print the generated OpenAPI 3.1 document without starting a runtime.
    Openapi,
    /// Run the one-time conversion of a pre-`catalog.toml` home into the current layout, then exit.
    ///
    /// Requires the original product database and an explicit confirmation of the target home. The
    /// runtime is never started, and a home that already published the current layout is refused.
    MigrateLegacyStorage {
        /// Absolute path of the Studio home to convert; must match `--studio-home`.
        #[arg(long, value_name = "ABSOLUTE_PATH")]
        confirm: PathBuf,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(Command::Openapi) => {
            println!("{}", pl_studio_server::openapi_json()?);
            return Ok(());
        }
        Some(Command::MigrateLegacyStorage { confirm }) => {
            return run_legacy_migration(cli.studio_home, confirm).await;
        }
        None => {}
    }
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();
    pl_studio_server::serve(ServerOptions {
        listen: cli.listen,
        studio_home: cli.studio_home,
    })
    .await
}

/// One-time, operator-invoked conversion of a pre-`catalog.toml` Studio home.
///
/// It prints an auditable summary of the durable migration report and exits non-zero when the
/// conversion failed. The Studio runtime is never started by this command.
async fn run_legacy_migration(
    studio_home: Option<PathBuf>,
    confirm: PathBuf,
) -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();
    let outcome = pl_studio_server::migrate_legacy_storage(studio_home, confirm).await?;
    println!(
        "legacy storage conversion: {}",
        if outcome.is_failure() {
            "failed"
        } else {
            "published"
        }
    );
    println!("  home:     {}", outcome.home.display());
    println!("  trigger:  explicit");
    println!("  phase:    {}", outcome.phase.unwrap_or("unknown"));
    println!("  report:   {}", outcome.report_path.display());
    println!(
        "  backup:   {}",
        outcome.backup_dir.as_deref().unwrap_or("-")
    );
    println!("  projects: {}", outcome.workspace_entries);
    println!("  settings: {}", outcome.settings_entries);
    println!(
        "  catalog:  {} entries (revision {})",
        outcome.catalog_entries,
        outcome
            .catalog_revision
            .map_or_else(|| "-".to_string(), |revision| revision.to_string())
    );
    println!(
        "  sessions: {} staged, {} verified",
        outcome.staged_sessions, outcome.verified_sessions
    );
    println!(
        "  calls:    retired={} verified={} schema={} database={}",
        outcome.calls_source_present,
        outcome.calls_verified,
        outcome.calls_source_schema_version,
        outcome.calls_source_database_id.as_deref().unwrap_or("-"),
    );
    println!(
        "  calls:    source model={} tool={} watermarks={} bodies={}/{}; destination model={} tool={}",
        outcome.calls_source_model_calls,
        outcome.calls_source_tool_calls,
        outcome.calls_source_watermarks,
        outcome.calls_source_bodies,
        outcome.calls_verified_bodies,
        outcome.calls_destination_model_calls,
        outcome.calls_destination_tool_calls,
    );
    if let Some(error) = outcome.error {
        anyhow::bail!("explicit legacy conversion failed: {error}");
    }
    println!(
        "  result:   published; no runtime was started (restart the app to use the new layout)"
    );
    Ok(())
}
