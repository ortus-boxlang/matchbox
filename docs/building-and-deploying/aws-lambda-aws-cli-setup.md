# AWS CLI Setup For MatchBox Lambda Deploys

This guide prepares your machine and AWS account for future `matchbox lambda ...` commands, especially creating or updating a single-file Lambda and printing its Function URL.

## 1. Install AWS CLI v2

MatchBox Lambda deploys will shell out to the `aws` CLI, so install AWS CLI version 2 and verify it is on your `PATH`.

On macOS, the official installer is:

```bash
curl -o /tmp/AWSCLIV2.pkg https://awscli.amazonaws.com/AWSCLIV2.pkg
sudo installer -pkg /tmp/AWSCLIV2.pkg -target /
aws --version
```

Expected shape:

```text
aws-cli/2.x.x ...
```

AWS CLI v1 and v2 both use the `aws` command name. If `aws --version` shows v1, upgrade or fix your `PATH` before testing deploys.

## 2. Configure Credentials

Recommended: use IAM Identity Center / SSO if your AWS account supports it.

```bash
aws configure sso --profile matchbox-dev
aws sso login --profile matchbox-dev
```

The wizard asks for:

- SSO start URL
- SSO region
- AWS account
- permission set / role
- default region
- output format

Use `json` for output format.

If you are using access keys instead of SSO:

```bash
aws configure --profile matchbox-dev
```

This writes config under:

```text
~/.aws/config
~/.aws/credentials
```

Do not use root account credentials.

## 3. Pick A Lambda Region

Choose a region that supports Lambda Function URLs. For example:

```bash
export AWS_PROFILE=matchbox-dev
export AWS_REGION=us-east-1
```

Or pass these explicitly when the deploy command exists:

```bash
matchbox lambda deploy Lambda.bx \
  --profile matchbox-dev \
  --region us-east-1 \
  ...
```

## 4. Verify Your Identity

Run:

```bash
aws sts get-caller-identity \
  --profile matchbox-dev \
  --region us-east-1
```

You should see JSON with:

```json
{
  "UserId": "...",
  "Account": "123456789012",
  "Arn": "..."
}
```

Keep the `Account` value handy.

## 5. Create A Lambda Execution Role

The first MatchBox deploy will require an existing Lambda execution role ARN. The deploy command will not create IAM roles in v1.

Create a trust policy:

```bash
cat > /tmp/matchbox-lambda-trust-policy.json <<'JSON'
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Principal": {
        "Service": "lambda.amazonaws.com"
      },
      "Action": "sts:AssumeRole"
    }
  ]
}
JSON
```

Create the role:

```bash
aws iam create-role \
  --role-name matchbox-lambda-execution \
  --assume-role-policy-document file:///tmp/matchbox-lambda-trust-policy.json \
  --profile matchbox-dev \
  --region us-east-1
```

Attach the basic CloudWatch Logs policy:

```bash
aws iam attach-role-policy \
  --role-name matchbox-lambda-execution \
  --policy-arn arn:aws:iam::aws:policy/service-role/AWSLambdaBasicExecutionRole \
  --profile matchbox-dev \
  --region us-east-1
```

Get the role ARN:

```bash
aws iam get-role \
  --role-name matchbox-lambda-execution \
  --query 'Role.Arn' \
  --output text \
  --profile matchbox-dev \
  --region us-east-1
```

Save that ARN. It will look like:

```text
arn:aws:iam::123456789012:role/matchbox-lambda-execution
```

IAM role propagation can take a few seconds. If Lambda create fails with a role-assumption error immediately after creating the role, wait briefly and retry.

## 6. Permissions Your AWS Principal Needs

The AWS profile you use for deploys needs permissions roughly equivalent to:

- `lambda:GetFunction`
- `lambda:CreateFunction`
- `lambda:UpdateFunctionCode`
- `lambda:UpdateFunctionConfiguration`
- `lambda:CreateFunctionUrlConfig`
- `lambda:GetFunctionUrlConfig`
- `lambda:AddPermission`
- `iam:PassRole` for the execution role ARN

If you create the role yourself with the commands above, your principal also needs:

- `iam:CreateRole`
- `iam:AttachRolePolicy`
- `iam:GetRole`

## 7. Expected MatchBox Deploy Shape

While release-embedded Lambda bootstrap stubs are still being wired in, the development command requires an explicit `--bootstrap` path. That file must be a Linux AWS custom runtime executable for the selected architecture. For real AWS testing on the default `arm64` architecture, it must be an ARM64 Linux binary named `bootstrap` inside the zip; the MatchBox deploy command handles the zip entry name.

### Function URL flags

- `--public` — Creates or updates a **public, unauthenticated** Function URL. Anyone on the internet can invoke it. The command adds the required `lambda:InvokeFunctionUrl` permission for principal `*`.
- `--url` — Creates or updates an **IAM-authenticated** Function URL. Callers must sign requests with valid AWS credentials.
- Omit both flags — No Function URL is created or modified.

You cannot use both `--public` and `--url` at the same time.

### Current development shape

```bash
matchbox lambda deploy Lambda.bx \
  --function matchbox-hello \
  --role arn:aws:iam::123456789012:role/matchbox-lambda-execution \
  --profile matchbox-dev \
  --region us-east-1 \
  --public \
  --bootstrap /path/to/linux-arm64-bootstrap
```

Once prebuilt stubs are embedded, a single-file deploy should look like:

```bash
matchbox lambda deploy Lambda.bx \
  --function matchbox-hello \
  --role arn:aws:iam::123456789012:role/matchbox-lambda-execution \
  --profile matchbox-dev \
  --region us-east-1 \
  --public
```

After the first create, updates should not need `--role`:

```bash
matchbox lambda deploy Lambda.bx \
  --function matchbox-hello \
  --profile matchbox-dev \
  --region us-east-1 \
  --public \
  --bootstrap /path/to/linux-arm64-bootstrap
```

The deploy command is expected to:

1. Package `Lambda.bx` and sibling support files.
2. Upload the zip.
3. Create or update the Lambda function.
4. Create or reuse a Function URL.
5. Print the URL.

## 8. Manual AWS CLI Equivalents

The MatchBox deploy command will automate AWS CLI calls like these:

```bash
aws lambda create-function \
  --function-name matchbox-hello \
  --runtime provided.al2023 \
  --handler bootstrap \
  --architectures arm64 \
  --role arn:aws:iam::123456789012:role/matchbox-lambda-execution \
  --zip-file fileb://dist/matchbox-hello.zip \
  --profile matchbox-dev \
  --region us-east-1
```

```bash
aws lambda update-function-code \
  --function-name matchbox-hello \
  --zip-file fileb://dist/matchbox-hello.zip \
  --profile matchbox-dev \
  --region us-east-1
```

```bash
aws lambda create-function-url-config \
  --function-name matchbox-hello \
  --auth-type NONE \
  --profile matchbox-dev \
  --region us-east-1
```

```bash
aws lambda add-permission \
  --function-name matchbox-hello \
  --statement-id FunctionURLAllowPublicAccess \
  --action lambda:InvokeFunctionUrl \
  --principal "*" \
  --function-url-auth-type NONE \
  --profile matchbox-dev \
  --region us-east-1
```

```bash
aws lambda get-function-url-config \
  --function-name matchbox-hello \
  --query FunctionUrl \
  --output text \
  --profile matchbox-dev \
  --region us-east-1
```

## Sources

- AWS CLI v2 install guide: https://docs.aws.amazon.com/cli/latest/userguide/getting-started-install.html
- AWS CLI SSO configuration: https://docs.aws.amazon.com/cli/latest/userguide/cli-configure-sso.html
- AWS CLI prerequisites and credential guidance: https://docs.aws.amazon.com/cli/latest/userguide/getting-started-prereqs.html
- Lambda Function URLs: https://docs.aws.amazon.com/lambda/latest/dg/urls-configuration.html
- AWS CLI `create-function-url-config`: https://docs.aws.amazon.com/cli/latest/reference/lambda/create-function-url-config.html
- AWS CLI `add-permission`: https://docs.aws.amazon.com/cli/latest/reference/lambda/add-permission.html
