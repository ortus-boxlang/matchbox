# Runtime Home, Configuration, and `.env` Files

MatchBox follows the BoxLang CLI here, with MatchBox names so the two runtimes never read each other's files.

| | BoxLang | MatchBox |
| :--- | :--- | :--- |
| Home directory | `~/.boxlang` | `~/.matchbox` |
| Home override | `BOXLANG_HOME`, `--home` | `MATCHBOX_HOME`, `--bx-home` |
| Config file | `<home>/config/boxlang.json` | `<home>/config/matchbox.json` |
| Project config | `./.boxlang.json` | `./.matchbox.json` |
| Config override | `BOXLANG_CONFIG`, `--bx-config` | `MATCHBOX_CONFIG`, `--bx-config` |
| User env file | `~/.box.env` | `~/.matchbox.env` |
| Project env file | `./.env` | `./.env` |

All of this runs at startup of the interpreter, the REPL, `--serve`, and compiled native binaries. It does not run on WASM or ESP32 targets, which have no home directory.

## `.env` files

Two files load on every start, in this order:

1. `~/.matchbox.env`: user-level, applies to every run for your account.
2. `./.env`: project-level, in the directory where you run `matchbox`.

Project values win over user values. File values also win over the OS environment, matching the BoxLang lookup order. With `--serve`, `<webroot>/.env` is loaded as well, like the BoxLang MiniServer.

```text
# ~/.matchbox.env
DB_HOST=localhost
OPENAI_API_KEY=sk-...

# ./.env
DB_HOST=staging.example.com
```

The format is `KEY=VALUE`, one per line. Lines that start with `#` are comments, an `export ` prefix is allowed, and quoting a value is optional. Read the values with `getSystemSetting()`:

```javascript
dbHost = getSystemSetting( "DB_HOST", "localhost" );
```

Never commit a `.env` with real secrets.

## The home directory

The home is chosen from `--bx-home`, then `MATCHBOX_HOME`, then `~/.matchbox`. Both `~/.matchbox.env` and `./.env` load first, so a project `.env` may set `MATCHBOX_HOME`.

On first run MatchBox creates:

```text
~/.matchbox/
  bin/  classes/  config/  global/  logs/  lib/  modules/
  config/matchbox.json      # seeded once, never overwritten
```

If the home cannot be created, MatchBox prints a warning and still runs the script. A bad `--bx-home` or `--bx-config` value is an error.

## `matchbox.json`

`matchbox.json` follows the BoxLang `boxlang.json` format. Comments are allowed. Two placeholders are expanded in string values: `${matchbox-home}` and `${user-dir}` (the working directory). Keys MatchBox does not support yet are ignored, so a BoxLang config can be reused.

The file is chosen from: `--bx-config`, then `MATCHBOX_CONFIG`, then `./.matchbox.json`, then `<home>/config/matchbox.json`. A file named by flag or variable must exist.

Supported keys:

| Key | Default | Meaning |
| :--- | :--- | :--- |
| `debugMode` | `false` | Raises the root log level to `DEBUG`. |
| `logging.logsDirectory` | `${matchbox-home}/logs` | Where log files are written. |
| `logging.maxLogDays` | `90` | Days to keep rotated files. `0` keeps them forever. |
| `logging.maxFileSize` | `100MB` | Size at which `matchbox.log` is rotated (`KB`, `MB`, `GB`). |
| `logging.totalCapSize` | `5GB` | Total size of all log files. Oldest rotated files are deleted first. |
| `logging.rootLevel` | `WARN` | `ERROR`, `WARN`, `INFO`, `DEBUG`, `TRACE` or `OFF`. |
| `logging.defaultEncoder` | `text` | `text` or `json`. |

```json
{
  "debugMode": false,
  "logging": { "rootLevel": "INFO", "defaultEncoder": "json" }
}
```

> The existing `mappings` support reads `boxlang.json` in the working directory. It does not read `matchbox.json` yet.

## Logs

The runtime writes to `<logsDirectory>/matchbox.log`. Text lines look like `2026-11-17 12:00:00.000 [INFO] message`. When the file passes `maxFileSize` it is renamed to `matchbox-<timestamp>.log` and a new one starts. Old files are pruned by `maxLogDays` and `totalCapSize`. `--verbose` and `--bx-debug` still print to stderr as before.
