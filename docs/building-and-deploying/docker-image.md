# Docker Image

MatchBox publishes a general-purpose Docker image to GitHub Container Registry:

```bash
ghcr.io/ortus-boxlang/matchbox
```

The image runs the main `matchbox` CLI as its entrypoint. That makes it useful for CI builds, direct script execution, bytecode compilation, WASM/native artifact generation, and webroot serving without installing MatchBox on the host.

---

## Tags

Stable releases publish:

| Tag | Description |
| :--- | :--- |
| `latest` | Most recent stable release |
| `vX.Y.Z` | Specific release version, matching the Git tag/Cargo version |

Develop snapshots publish:

| Tag | Description |
| :--- | :--- |
| `develop` | Latest build from the `develop` branch |
| `snapshot` | Rolling snapshot alias |
| `be` | Rolling BE/development image alias |

Pull the stable image:

```bash
docker pull ghcr.io/ortus-boxlang/matchbox:latest
```

Pull the current develop image:

```bash
docker pull ghcr.io/ortus-boxlang/matchbox:develop
```

---

## Basic Usage

The container workdir is `/app`. Mount your project there and pass normal MatchBox arguments:

```bash
docker run --rm -v "$PWD:/app" ghcr.io/ortus-boxlang/matchbox:latest --help
docker run --rm -v "$PWD:/app" ghcr.io/ortus-boxlang/matchbox:latest --version
docker run --rm -v "$PWD:/app" ghcr.io/ortus-boxlang/matchbox:latest my_script.bxs
```

Because `matchbox` is the entrypoint, arguments after the image name are passed directly to MatchBox.

---

## Build Artifacts

Compile bytecode into the mounted project directory:

```bash
docker run --rm -v "$PWD:/app" ghcr.io/ortus-boxlang/matchbox:latest --build my_script.bxs
```

Build a WASM/WASI artifact:

```bash
docker run --rm -v "$PWD:/app" ghcr.io/ortus-boxlang/matchbox:latest --target wasm my_service.bxs
```

Build a native Linux artifact:

```bash
docker run --rm -v "$PWD:/app" ghcr.io/ortus-boxlang/matchbox:latest --target native my_app.bxs
```

Native artifacts produced inside the container target Linux. Use the release binaries or source builds when you need macOS or Windows native artifacts.

---

## Webroot Server

To serve `.bxm` templates and static files, bind the server to `0.0.0.0` inside the container and publish the port:

```bash
docker run --rm \
  -p 8080:8080 \
  -v "$PWD/www:/app" \
  ghcr.io/ortus-boxlang/matchbox:latest \
  --serve --host 0.0.0.0 --port 8080 --webroot /app
```

Then open `http://localhost:8080`.

---

## CI Example

Use the image in a CI job when you only need the MatchBox CLI:

```bash
docker run --rm -v "$PWD:/app" ghcr.io/ortus-boxlang/matchbox:latest --build src/main.bxs
docker run --rm -v "$PWD:/app" ghcr.io/ortus-boxlang/matchbox:latest --target wasm src/main.bxs
```

For release automation, pin a version tag such as `v0.6.4` instead of `latest`.
