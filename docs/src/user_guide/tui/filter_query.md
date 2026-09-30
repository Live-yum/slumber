# Data Filtering & Querying

This fork evaluates response filters in-process using Rust JaQ. Focus the response pane and press `/`. Enter `.data.list`, `.items | map(.name)` or `jq '.data.list'`. A bare `jq` prettifies JSON; `jq -r '.token'` returns raw text; `jq -c '.'` uses compact JSON. The executable does not start jq or a shell for these filters.

The selected raw/transformed response is used as input. Existing query history and default-query configuration remain available. The native engine does not implement every flag of the external jq command-line program.

## Exporting data

Press `:` and enter `save PATH` or `tee PATH`. The path may contain spaces and Windows backslashes; surrounding double quotes are optional. The legacy `tee > PATH` spelling is also accepted. Export creates a complete new file atomically and refuses to overwrite existing files. Use the Save Body dialog for confirmed replacement of an existing file.

Native `cat`, `head -c N` and `head -n N` work without installed executables. Use the Copy Body action to copy through the terminal clipboard protocol rather than running pbcopy/xclip.

## Default query

```yaml
commands:
  default_query:
    json: jq
```

Default queries accept a string or the existing MIME map. Prefer native JaQ filters for portable collections.

## Explicit external integrations

Normal mode retains external shell execution only when prefixed with `!`, for example `!grep something`. It uses the existing commands.shell setting. The selected program must be installed by the user. Portable mode rejects `!` commands before spawning a process and does not silently fall back to external programs. Arbitrary Python/Node/Java scripts are not embedded runtimes.
