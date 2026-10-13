use clap::Parser;
use matchbox_server::{Args, run_server};

#[tokio::main]
async fn main() {
    let args = Args::parse();
    run_server(args).await;
}
