# ESP32 Compile Pipeline

## Direction

The ESP32 target should reuse the shared MatchBox parser/frontend while running a stricter embedded build pipeline on top.

It should not use a totally separate parser.

## Embedded Build Steps

When `--target esp32` is selected, the build pipeline should:

1. parse the BoxLang entry script with the shared parser
2. run embedded profile validation
3. discover `app/**/*.bxm` and `app/**/*.bxs`
4. convert file paths into extensionless embedded routes
5. emit an embedded route manifest
6. tree-shake aggressively for the embedded profile
7. hand artifacts to the bundled ESP32 runner

## Current State

The CLI now discovers the embedded `app/` directory and generates a JSON manifest for routes such as:

- `app/index.bxm` -> `GET /`
- `app/status.bxm` -> `GET /status`
- `app/print.post.bxs` -> `POST /print`
- `app/printer/[id].bxm` -> `GET /printer/:id`

The next steps are:

- compile `.bxm` files into embedded render handlers
- compile `.bxs` route scripts into embedded handler units
- make the runner consume the manifest directly
- enforce the lean request scopes `url`, `form`, `request`, and `cgi`
