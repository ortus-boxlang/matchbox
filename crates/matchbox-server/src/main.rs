use clap::Parser;
use matchbox_server::{Args, run_server};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    matchbox_utility::bootstrap::bootstrap(None, None)?;
    run_server(args).await
}
