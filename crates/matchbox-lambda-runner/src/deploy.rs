use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::packaging::{BootstrapStubs, LambdaArchitecture, write_package_zip};

#[derive(Debug, Clone)]
pub struct DeployOptions {
    pub source_path: PathBuf,
    pub zip_path: PathBuf,
    pub function_name: String,
    pub role: Option<String>,
    pub profile: Option<String>,
    pub region: Option<String>,
    pub architecture: LambdaArchitecture,
    pub memory: Option<u16>,
    pub timeout: Option<u16>,
    pub function_url: Option<FunctionUrlAuth>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FunctionUrlAuth {
    None,
    AwsIam,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeployResult {
    pub created: bool,
    pub function_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AwsOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

pub trait AwsCli {
    fn run(&mut self, args: &[String]) -> Result<AwsOutput>;
}

pub struct ProcessAwsCli;

impl AwsCli for ProcessAwsCli {
    fn run(&mut self, args: &[String]) -> Result<AwsOutput> {
        let output = Command::new("aws")
            .args(args)
            .output()
            .with_context(|| "failed to run aws CLI")?;
        Ok(AwsOutput {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).trim().to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        })
    }
}

pub fn deploy_with_cli(
    options: &DeployOptions,
    stubs: BootstrapStubs<'_>,
    cli: &mut dyn AwsCli,
) -> Result<DeployResult> {
    write_package_zip(
        &options.source_path,
        &options.zip_path,
        options.architecture,
        stubs,
    )?;

    let exists = aws_success(cli, command(options, ["lambda", "get-function"]))?;
    let created = if exists {
        run_required(
            cli,
            command(options, ["lambda", "update-function-code"])
                .arg("--zip-file")
                .arg(zip_file_arg(&options.zip_path)),
            "failed to update Lambda function code",
        )?;
        false
    } else {
        let role = options
            .role
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("--role is required when creating a Lambda function"))?;
        run_required(
            cli,
            command(options, ["lambda", "create-function"])
                .arg("--runtime")
                .arg("provided.al2023")
                .arg("--handler")
                .arg("bootstrap")
                .arg("--architectures")
                .arg(architecture_arg(options.architecture))
                .arg("--role")
                .arg(role)
                .arg("--zip-file")
                .arg(zip_file_arg(&options.zip_path)),
            "failed to create Lambda function",
        )?;
        true
    };

    if options.memory.is_some() || options.timeout.is_some() {
        let mut cmd = command(options, ["lambda", "update-function-configuration"]);
        if let Some(memory) = options.memory {
            cmd = cmd.arg("--memory-size").arg(memory.to_string());
        }
        if let Some(timeout) = options.timeout {
            cmd = cmd.arg("--timeout").arg(timeout.to_string());
        }
        run_required(cli, cmd, "failed to update Lambda function configuration")?;
    }

    let function_url = if let Some(auth) = options.function_url {
        Some(ensure_function_url(options, auth, cli)?)
    } else {
        None
    };

    Ok(DeployResult {
        created,
        function_url,
    })
}

fn ensure_function_url(
    options: &DeployOptions,
    auth: FunctionUrlAuth,
    cli: &mut dyn AwsCli,
) -> Result<String> {
    let existing = run_optional(
        cli,
        command(options, ["lambda", "get-function-url-config"])
            .arg("--query")
            .arg("FunctionUrl")
            .arg("--output")
            .arg("text"),
    )?;
    if let Some(url) = non_empty_stdout(existing) {
        let existing_auth = run_optional(
            cli,
            command(options, ["lambda", "get-function-url-config"])
                .arg("--query")
                .arg("AuthType")
                .arg("--output")
                .arg("text"),
        )?;
        let needs_update = non_empty_stdout(existing_auth)
            .map(|value| value != function_url_auth_arg(auth))
            .unwrap_or(false);
        if needs_update {
            run_required(
                cli,
                command(options, ["lambda", "update-function-url-config"])
                    .arg("--auth-type")
                    .arg(function_url_auth_arg(auth)),
                "failed to update Lambda function URL auth type",
            )?;
        }
        if auth == FunctionUrlAuth::None {
            ensure_public_function_url_permissions(options, cli)?;
        }
        return Ok(url);
    }

    run_required(
        cli,
        command(options, ["lambda", "create-function-url-config"])
            .arg("--auth-type")
            .arg(function_url_auth_arg(auth)),
        "failed to create Lambda function URL",
    )?;

    if auth == FunctionUrlAuth::None {
        ensure_public_function_url_permissions(options, cli)?;
    }

    let created = run_required(
        cli,
        command(options, ["lambda", "get-function-url-config"])
            .arg("--query")
            .arg("FunctionUrl")
            .arg("--output")
            .arg("text"),
        "failed to read Lambda function URL",
    )?;
    non_empty_stdout(Some(created))
        .ok_or_else(|| anyhow::anyhow!("AWS CLI returned an empty Function URL"))
}

fn ensure_public_function_url_permissions(
    options: &DeployOptions,
    cli: &mut dyn AwsCli,
) -> Result<()> {
    add_idempotent_permission(
        cli,
        command(options, ["lambda", "add-permission"])
            .arg("--statement-id")
            .arg("FunctionURLAllowPublicAccess")
            .arg("--action")
            .arg("lambda:InvokeFunctionUrl")
            .arg("--principal")
            .arg("*")
            .arg("--function-url-auth-type")
            .arg("NONE"),
        "failed to add Function URL invoke-url permission",
    )?;
    add_idempotent_permission(
        cli,
        command(options, ["lambda", "add-permission"])
            .arg("--statement-id")
            .arg("FunctionURLInvokeAllowPublicAccess")
            .arg("--action")
            .arg("lambda:InvokeFunction")
            .arg("--principal")
            .arg("*")
            .arg("--invoked-via-function-url"),
        "failed to add Function URL invoke-function permission",
    )
}

fn add_idempotent_permission(
    cli: &mut dyn AwsCli,
    cmd: AwsCommand,
    context: &str,
) -> Result<()> {
    let permission = run_optional(cli, cmd)?;
    if let Some(output) = permission {
        if !output.success && !output.stderr.contains("ResourceConflictException") {
            bail!("{}: {}", context, output.stderr);
        }
    }
    Ok(())
}

fn command<const N: usize>(options: &DeployOptions, base: [&str; N]) -> AwsCommand {
    let mut args = base.into_iter().map(str::to_string).collect::<Vec<_>>();
    args.push("--function-name".to_string());
    args.push(options.function_name.clone());
    append_common_options(options, &mut args);
    AwsCommand { args }
}

fn append_common_options(options: &DeployOptions, args: &mut Vec<String>) {
    if let Some(profile) = &options.profile {
        args.push("--profile".to_string());
        args.push(profile.clone());
    }
    if let Some(region) = &options.region {
        args.push("--region".to_string());
        args.push(region.clone());
    }
}

#[derive(Debug, Clone)]
struct AwsCommand {
    args: Vec<String>,
}

impl AwsCommand {
    fn arg(mut self, value: impl Into<String>) -> Self {
        self.args.push(value.into());
        self
    }
}

fn aws_success(cli: &mut dyn AwsCli, cmd: AwsCommand) -> Result<bool> {
    Ok(cli.run(&cmd.args)?.success)
}

fn run_optional(cli: &mut dyn AwsCli, cmd: AwsCommand) -> Result<Option<AwsOutput>> {
    Ok(Some(cli.run(&cmd.args)?))
}

fn run_required(cli: &mut dyn AwsCli, cmd: AwsCommand, context: &str) -> Result<AwsOutput> {
    let output = cli.run(&cmd.args)?;
    if output.success {
        Ok(output)
    } else {
        bail!("{}: {}", context, output.stderr)
    }
}

fn non_empty_stdout(output: Option<AwsOutput>) -> Option<String> {
    output
        .filter(|output| output.success)
        .map(|output| output.stdout.trim().to_string())
        .filter(|value| !value.is_empty() && value != "None")
}

fn zip_file_arg(path: &std::path::Path) -> String {
    format!("fileb://{}", path.display())
}

fn architecture_arg(architecture: LambdaArchitecture) -> &'static str {
    match architecture {
        LambdaArchitecture::Arm64 => "arm64",
        LambdaArchitecture::X86_64 => "x86_64",
    }
}

fn function_url_auth_arg(auth: FunctionUrlAuth) -> &'static str {
    match auth {
        FunctionUrlAuth::None => "NONE",
        FunctionUrlAuth::AwsIam => "AWS_IAM",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct MockAwsCli {
        calls: Vec<Vec<String>>,
        outputs: Vec<AwsOutput>,
    }

    impl AwsCli for MockAwsCli {
        fn run(&mut self, args: &[String]) -> Result<AwsOutput> {
            self.calls.push(args.to_vec());
            if self.outputs.is_empty() {
                bail!("unexpected aws call: {:?}", args);
            }
            Ok(self.outputs.remove(0))
        }
    }

    #[test]
    fn creates_missing_function_and_public_url() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Lambda.bx"), "class {}").unwrap();
        let options = options(dir.path().join("Lambda.bx"), dir.path().join("lambda.zip"));
        let mut cli = MockAwsCli {
            outputs: vec![
                fail("not found"),
                ok("{}"),
                ok("{}"),
                fail("not found"),
                ok("{}"),
                ok("{}"),
                ok("{}"),
                ok("https://abc.lambda-url.us-east-1.on.aws/"),
            ],
            ..Default::default()
        };

        let result = deploy_with_cli(&options, stubs(), &mut cli).unwrap();

        assert!(result.created);
        assert_eq!(
            result.function_url.as_deref(),
            Some("https://abc.lambda-url.us-east-1.on.aws/")
        );
        assert!(
            cli.calls
                .iter()
                .any(|call| call.contains(&"create-function".to_string()))
        );
        assert!(
            cli.calls
                .iter()
                .any(|call| call.contains(&"FunctionURLAllowPublicAccess".to_string()))
        );
        assert!(
            cli.calls
                .iter()
                .any(|call| call.contains(&"FunctionURLInvokeAllowPublicAccess".to_string()))
        );
    }

    #[test]
    fn repairs_public_permissions_for_existing_function_url() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Lambda.bx"), "class {}").unwrap();
        let options = options(dir.path().join("Lambda.bx"), dir.path().join("lambda.zip"));
        let mut cli = MockAwsCli {
            outputs: vec![
                ok("{}"),
                ok("{}"),
                ok("{}"),
                ok("https://abc.lambda-url.us-east-1.on.aws/"),
                ok("NONE"),
                ok("{}"),
                ok("{}"),
            ],
            ..Default::default()
        };

        let result = deploy_with_cli(&options, stubs(), &mut cli).unwrap();

        assert!(!result.created);
        assert_eq!(
            result.function_url.as_deref(),
            Some("https://abc.lambda-url.us-east-1.on.aws/")
        );
        assert!(
            cli.calls
                .iter()
                .any(|call| call.contains(&"FunctionURLAllowPublicAccess".to_string()))
        );
        assert!(
            cli.calls
                .iter()
                .any(|call| call.contains(&"FunctionURLInvokeAllowPublicAccess".to_string()))
        );
        assert!(
            !cli.calls
                .iter()
                .any(|call| call.contains(&"update-function-url-config".to_string()))
        );
    }

    #[test]
    fn updates_existing_url_auth_from_iam_to_none() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Lambda.bx"), "class {}").unwrap();
        let options = options(dir.path().join("Lambda.bx"), dir.path().join("lambda.zip"));
        let mut cli = MockAwsCli {
            outputs: vec![
                ok("{}"),
                ok("{}"),
                ok("{}"),
                ok("https://abc.lambda-url.us-east-1.on.aws/"),
                ok("AWS_IAM"),
                ok("{}"),
                ok("{}"),
                ok("{}"),
            ],
            ..Default::default()
        };

        let result = deploy_with_cli(&options, stubs(), &mut cli).unwrap();

        assert!(!result.created);
        assert_eq!(
            result.function_url.as_deref(),
            Some("https://abc.lambda-url.us-east-1.on.aws/")
        );
        assert!(
            cli.calls
                .iter()
                .any(|call| call.contains(&"update-function-url-config".to_string()))
        );
        assert!(
            cli.calls
                .iter()
                .any(|call| call.contains(&"FunctionURLAllowPublicAccess".to_string()))
        );
        assert!(
            cli.calls
                .iter()
                .any(|call| call.contains(&"FunctionURLInvokeAllowPublicAccess".to_string()))
        );
    }

    #[test]
    fn updates_existing_function_without_role() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Lambda.bx"), "class {}").unwrap();
        let mut options = options(dir.path().join("Lambda.bx"), dir.path().join("lambda.zip"));
        options.role = None;
        options.function_url = None;
        options.memory = None;
        options.timeout = None;
        let mut cli = MockAwsCli {
            outputs: vec![ok("{}"), ok("{}")],
            ..Default::default()
        };

        let result = deploy_with_cli(&options, stubs(), &mut cli).unwrap();

        assert!(!result.created);
        assert!(
            cli.calls
                .iter()
                .any(|call| call.contains(&"update-function-code".to_string()))
        );
    }

    fn options(source_path: PathBuf, zip_path: PathBuf) -> DeployOptions {
        DeployOptions {
            source_path,
            zip_path,
            function_name: "matchbox-test".to_string(),
            role: Some("arn:aws:iam::123456789012:role/matchbox-lambda".to_string()),
            profile: Some("matchbox-dev".to_string()),
            region: Some("us-east-1".to_string()),
            architecture: LambdaArchitecture::Arm64,
            memory: Some(128),
            timeout: Some(15),
            function_url: Some(FunctionUrlAuth::None),
        }
    }

    fn stubs() -> BootstrapStubs<'static> {
        BootstrapStubs {
            arm64: b"arm64",
            x86_64: b"x86",
        }
    }

    fn ok(stdout: &str) -> AwsOutput {
        AwsOutput {
            success: true,
            stdout: stdout.to_string(),
            stderr: String::new(),
        }
    }

    fn fail(stderr: &str) -> AwsOutput {
        AwsOutput {
            success: false,
            stdout: String::new(),
            stderr: stderr.to_string(),
        }
    }
}
