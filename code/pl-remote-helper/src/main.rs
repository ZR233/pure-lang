#[tokio::main]
async fn main() {
    if let Err(error) = pl_remote_helper::run_stdio().await {
        eprintln!("pl-remote-helper failed: {error}");
        std::process::exit(1);
    }
}
