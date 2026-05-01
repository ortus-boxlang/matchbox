use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let task_root = std::env::var("LAMBDA_TASK_ROOT").unwrap_or_else(|_| "/var/task".to_string());
    let runtime_api = std::env::var("AWS_LAMBDA_RUNTIME_API")
        .map_err(|_| anyhow::anyhow!("AWS_LAMBDA_RUNTIME_API is not set"))?;
    matchbox_lambda_runner::runtime_api::run_runtime_api_loop(
        &PathBuf::from(task_root),
        &runtime_api,
    )
}
