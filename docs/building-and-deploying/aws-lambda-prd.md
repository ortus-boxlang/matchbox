# MatchBox AWS Lambda Runtime PRD

## Problem Statement

MatchBox can already target native binaries, WASI/WebAssembly, browser runtimes, app servers, and embedded devices, but it does not yet provide a first-class AWS Lambda deployment path. A BoxLang developer who wants to run MatchBox on Lambda must manually assemble a custom runtime, package files correctly, create or update the Lambda function, configure a Function URL, and understand the AWS Runtime API.

The desired experience is a MatchBox-native Lambda runtime that preserves the familiar BoxLang AWS Lambda programming model while keeping deployment lightweight and CLI-driven. Developers should be able to write `Lambda.bx`, optionally include `Application.bx`, test locally, package a Lambda zip, deploy it through the AWS CLI, and receive the Function URL without using the AWS console.

## Solution

Add a self-contained MatchBox AWS Lambda runtime and CLI workflow.

The runtime will use the existing BoxLang AWS Lambda convention: a `Lambda.bx` class containing a `run( event, context, response )` function. The incoming Lambda event is passed through as a BoxLang struct. The `context` argument is a MatchBox native object that imitates the AWS Java Lambda context method contract. The `response` argument is a mutable struct initialized with Lambda proxy-response defaults.

The MatchBox CLI will expose dedicated Lambda subcommands:

- `matchbox lambda package`
- `matchbox lambda invoke`
- `matchbox lambda deploy`

The package command builds a zip containing a prebuilt Lambda `bootstrap` runner and the BoxLang source files. The invoke command runs the same runtime path locally against an event JSON file. The deploy command shells out to the AWS CLI to create or update the function, upload the zip, optionally create a Function URL, and print the resulting URL.

## User Stories

1. As a BoxLang developer, I want to deploy a `Lambda.bx` file to AWS Lambda, so that I can run MatchBox code without creating a full web server.
2. As a BoxLang developer, I want `Lambda.bx` to contain a class with `run( event, context, response )`, so that MatchBox Lambda code feels compatible with the existing BoxLang Lambda starter.
3. As a BoxLang developer, I want the raw AWS event passed into `run()`, so that I can handle Function URL, API Gateway, ALB, and direct invocation payloads myself.
4. As a BoxLang developer, I want a default response struct passed into `run()`, so that I can mutate status, headers, cookies, and body directly.
5. As a BoxLang developer, I want non-null return values from `run()` to become the response body, so that simple functions can return values without mutating the response struct.
6. As a BoxLang developer, I want full response-looking structs returned from `run()` to replace the response, so that advanced handlers can build the complete Lambda response in one value.
7. As a BoxLang developer, I want struct and array response bodies to be JSON-encoded, so that common API responses work without manual serialization.
8. As a BoxLang developer, I want byte bodies to be base64-encoded and marked as base64 responses, so that binary responses are supported.
9. As a BoxLang developer, I want `Application.bx` lifecycle hooks to run when present, so that existing Lambda starter projects can move toward MatchBox with fewer changes.
10. As a BoxLang developer, I want `onApplicationStart()` to run once per cold start, so that I can initialize app-level state.
11. As a BoxLang developer, I want `onRequestStart()` and `onRequestEnd()` to run around each invocation, so that I can implement request setup and teardown.
12. As a BoxLang developer, I want `onError()` to be called for runtime errors, so that I can centralize error handling.
13. As a BoxLang developer, I want URI class routing to work like the BoxLang Java Lambda runner, so that `/products` can resolve to `Products.bx` when that class exists.
14. As a BoxLang developer, I want URI class routing to fall back to `Lambda.bx`, so that single-class Lambdas still work for all paths.
15. As a BoxLang developer, I want `context.getAwsRequestId()` and similar methods, so that code written against the AWS Java Lambda context has a MatchBox-compatible shape.
16. As a BoxLang developer, I want `context.getLogger().log()` to write to Lambda logs, so that I can inspect output in CloudWatch.
17. As a BoxLang developer, I want local invocation from an event JSON file, so that I can test the runtime contract before deploying.
18. As a BoxLang developer, I want starter-style project packaging, so that `src/main/bx` and `src/resources` projects deploy cleanly.
19. As a BoxLang developer, I want single-file packaging, so that small Lambdas can be deployed from one `Lambda.bx` file.
20. As a BoxLang developer, I want sibling `Application.bx`, `boxlang.json`, `boxlang_modules`, and top-level `.bx` files included automatically for single-file deploys, so that compatibility and URI routing work without extra flags.
21. As a BoxLang developer, I want the CLI to use prebuilt Lambda runner stubs, so that I do not need to configure a Rust cross-compilation toolchain just to deploy.
22. As a BoxLang developer, I want ARM64 to be the default architecture, so that Lambda deployments default to a cost-efficient native target.
23. As a BoxLang developer, I want to choose x86_64 explicitly, so that I can deploy to environments that require it.
24. As a BoxLang developer, I want deployment to shell out to the AWS CLI, so that existing AWS profiles, regions, SSO, and credentials work naturally.
25. As a BoxLang developer, I want deploy to create the function when missing and update it when present, so that one command handles the common lifecycle.
26. As a BoxLang developer, I want deploy to upload the generated zip, so that I do not need separate AWS CLI commands for code updates.
27. As a BoxLang developer, I want deploy to create or reuse a Function URL, so that I can immediately call the deployed Lambda over HTTP.
28. As a BoxLang developer, I want deploy to print the Function URL, so that I can use the deployed endpoint immediately.
29. As an operations-focused user, I want first create to require an existing IAM role ARN, so that MatchBox does not make account-level IAM decisions for me.
30. As a contributor, I want the Lambda runtime isolated in its own crate, so that it can be tested and evolved independently from the app server runtime.

## Implementation Decisions

- Create a dedicated Lambda runtime crate that owns source discovery, lifecycle handling, event invocation, context/native logger objects, response normalization, URI class routing, and the AWS Runtime API loop.
- The Lambda runtime must be independent from the existing MatchBox app server. It should not depend on `boxlang.web` routing or `matchbox-server` request dispatch.
- The runtime entrypoint is a BoxLang class named `Lambda.bx` with a `run( event, context, response )` method.
- The default deployed Lambda root follows AWS custom runtime conventions. `Lambda.bx`, `Application.bx`, `boxlang.json`, and URI-routed classes are expected at the zip root.
- The runtime should also support local developer fallbacks for starter-style source layouts.
- `BOXLANG_LAMBDA_CLASS` overrides the default Lambda class path.
- `BOXLANG_LAMBDA_DEBUGMODE` enables runtime debug logging.
- `BOXLANG_LAMBDA_CONFIG` overrides config discovery.
- If no config override exists, the runtime looks for `boxlang.json` in the Lambda root.
- The default response contains status code `200`, JSON content type, permissive CORS origin, empty body, and empty cookies.
- Response normalization coerces `statusCode`, `headers`, `cookies`, `body`, and `isBase64Encoded` into a Lambda proxy-compatible response.
- The runtime passes the raw Lambda event into BoxLang without converting it into the MatchBox web server request model.
- URI class routing inspects known AWS event shapes for an HTTP path, maps the first path segment to a PascalCase `.bx` class, and falls back to `Lambda.bx` when no matching class exists.
- `x-bx-function` method dispatch is intentionally excluded from v1.
- `Application.bx` is included in v1, with a compatibility-focused lifecycle subset.
- The lifecycle should include cold-start application initialization and per-invocation request start/end/error hooks.
- The MatchBox context object should imitate the AWS Java Lambda context at the method level rather than exposing only struct fields.
- The logger returned by the context writes to stdout/stderr-compatible Lambda logs and accepts a message plus an optional string level.
- CLI package behavior supports both starter-style directories and single-file `Lambda.bx` input.
- Single-file packaging automatically includes sibling top-level `.bx` files, sibling `Application.bx`, sibling `boxlang.json`, and sibling `boxlang_modules`.
- Directory packaging flattens starter-style `src/main/bx` contents into the zip root and places starter-style resources at the zip root.
- The normal packaging path uses prebuilt Lambda `bootstrap` runner stubs for ARM64 and x86_64.
- ARM64 is the default Lambda architecture; x86_64 is selectable.
- v1 packages source files and compiles/caches them on cold start or first use. Precompiled bytecode packaging is a later optimization.
- Deploy shells out to the AWS CLI instead of linking an AWS SDK into MatchBox.
- Deploy owns Lambda function creation/update, code upload, optional Function URL configuration, and URL reporting.
- Deploy does not create IAM roles in v1. First create requires a role ARN.

## Testing Decisions

- Tests should focus on external runtime behavior: given source files, event JSON, environment/config options, and a context seed, the runtime returns the expected normalized Lambda response.
- Unit tests should cover response normalization for strings, structs, arrays, numbers, booleans, null, and bytes.
- Unit tests should cover context native object methods and logger behavior.
- Unit tests should cover event path extraction for API Gateway v2, Function URLs, API Gateway v1, ALB, and direct invocations.
- Unit tests should cover URI class routing, including fallback to `Lambda.bx`.
- Unit tests should cover package file selection for starter-style directories and single-file inputs.
- Integration tests should exercise local invocation against sample Lambda event JSON files.
- Integration tests should verify `Application.bx` lifecycle hook ordering and error hook behavior from observable output or response changes.
- CLI tests should verify generated AWS CLI command construction without requiring real AWS credentials.
- Live AWS deployment tests are out of scope for normal CI, but the deploy command should have a dry-run or debug path that is easy to validate locally.
- Prior art includes the existing integration tests for script execution, web runtime behavior, and WASI HTTP packaging.

## Out of Scope

- Creating IAM roles or IAM policies.
- Full AWS SDK integration.
- `x-bx-function` alternate method dispatch.
- Full Java object compatibility for the AWS context object.
- WebSocket Lambda support.
- `boxlang.web` app-server routing inside Lambda.
- Precompiled bytecode Lambda packages.
- Container-image Lambda deployments.
- SAM/CloudFormation project generation.
- Response streaming.
- Full parity with every BoxLang JVM application lifecycle hook.
- Recursive packaging of arbitrary project directories outside the explicit starter/single-file rules.

## Further Notes

This feature should optimize for a low-friction native MatchBox Lambda path while staying close to the existing BoxLang AWS Lambda runner where it matters: `Lambda.bx`, `run( event, context, response )`, `Application.bx`, response shape, context method names, and URI class routing.

The first useful implementation slice should be local invocation from source files. Once local invocation is stable, packaging can add prebuilt `bootstrap` stubs. Deployment can then layer on AWS CLI shell-out with create/update/Function URL behavior.

The compatibility goal is practical rather than exact JVM emulation. MatchBox should preserve the developer-facing contract, but avoid pulling Java-specific assumptions into the native runtime.
