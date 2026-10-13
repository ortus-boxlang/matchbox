# SQLite datasource examples

These examples use the native datasource support that is enabled with the `bif-datasource` feature.

## Required build feature

```bash
cargo build --features bif-datasource
```

For a native host run, the project binary can then execute the example scripts, for example:

```bash
cargo run --features bif-datasource -- docs/examples/sqlite_datasource/programmatic_registration.bxs
```

## Programmatic registration

`programmatic_registration.bxs` creates an in-memory SQLite datasource with `datasourceRegister()`, then runs a query via `queryExecute()`.

## TOML configuration

`toml_config/` contains a project directory with a `matchbox.toml` file that defines `[datasources.db]` using SQLite. The script `main.bxs` reads the datasource by name without calling `datasourceRegister()`.

```bash
cd docs/examples/sqlite_datasource/toml_config
cargo run --features bif-datasource -- main.bxs
```
