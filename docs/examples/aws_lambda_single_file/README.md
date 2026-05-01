# Single-File AWS Lambda

This example is the smallest MatchBox Lambda shape:

```text
Lambda.bx
event.json
```

`Lambda.bx` defines a class with:

```boxlang
function run(event, context, response)
```

The runtime passes the raw AWS Lambda event into `event`, an AWS-style context object into `context`, and a mutable Lambda proxy response struct into `response`.

## Local Invoke

From the repo root:

```bash
cargo run -- lambda invoke docs/examples/aws_lambda_single_file/Lambda.bx \
  --event docs/examples/aws_lambda_single_file/event.json
```

Expected response shape:

```json
{
  "body": "{\"message\":\"Hello from Lambda\",\"path\":\"/\",\"requestId\":\"local-request\"}",
  "cookies": [],
  "headers": {
    "Access-Control-Allow-Origin": "*",
    "Content-Type": "application/json"
  },
  "isBase64Encoded": false,
  "statusCode": 200
}
```

## AWS CLI Setup

Before deploying, follow:

```text
docs/building-and-deploying/aws-lambda-aws-cli-setup.md
```

You need:

- AWS CLI v2
- a configured AWS profile
- a region
- an existing Lambda execution role ARN

## Deploy

Current development builds still require an explicit Linux Lambda bootstrap binary:

```bash
cargo run -- lambda deploy docs/examples/aws_lambda_single_file/Lambda.bx \
  --function matchbox-single-file-demo \
  --role arn:aws:iam::123456789012:role/matchbox-lambda-execution \
  --profile matchbox-dev \
  --region us-east-1 \
  --public \
  --bootstrap /path/to/linux-arm64-bootstrap
```

Use `--public` for an unauthenticated Function URL that anyone can invoke. Use `--url` instead if you want IAM-authenticated access.

After embedded release stubs are added, `--bootstrap` should go away.

If deploy succeeds, the command prints the Function URL. Test it with:

```bash
curl "https://your-url.lambda-url.us-east-1.on.aws/?name=AWS"
```

Expected body:

```json
{
  "message": "Hello from AWS",
  "requestId": "...",
  "path": "/"
}
```

