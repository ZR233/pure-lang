use std::net::SocketAddr;
use std::path::PathBuf;

use clap::{Parser, Subcommand};
use pl_studio_server::{DEFAULT_LISTEN, ServerOptions};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(
    name = "pl-studio-server",
    version,
    about = "Pure Studio loopback HTTP API"
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
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    if matches!(cli.command, Some(Command::Openapi)) {
        println!("{}", pl_studio_server::openapi_json()?);
        return Ok(());
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
