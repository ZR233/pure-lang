mod server;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    server::run(server::ServerOptions::parse(std::env::args_os().skip(1))?).await
}
