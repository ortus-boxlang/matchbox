use matchbox_server::{run_server, Args};
use clap::Parser;

#[tokio::main]
async fn main() {
    let args = Args::parse();
    run_server(args).await;
}
