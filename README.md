# Slumber

[![Test CI](https://github.com/github/docs/actions/workflows/test.yml/badge.svg)](https://github.com/LucasPickering/slumber/actions)
[![crates.io](https://img.shields.io/crates/v/slumber.svg)](https://crates.io/crates/slumber)
[![Sponsor](https://img.shields.io/github/sponsors/LucasPickering?logo=github)](https://github.com/sponsors/LucasPickering)

- [Installation](https://slumber.lucaspickering.me/install.html)
- [Docs](https://slumber.lucaspickering.me/)
- [Changelog](https://github.com/lucasPickering/slumber/releases)

![Slumber example](./docs/src/images/demo.gif)

Slumber is a TUI (terminal user interface) HTTP client. Define, execute, and share configurable HTTP requests. Slumber is built on some basic principles:

- It will remain free to use forever
- You own your data: all configuration and data is stored locally and can be checked into version control
- It will never be [enshittified](https://en.wikipedia.org/wiki/Enshittification)

## Features

- Usable as a TUI, CLI, or [Python package](https://slumber.lucaspickering.me/integration/python.html)
- Source-first configuration, for easy persistence and sharing
- [Import from external formats (e.g. Insomnia)](https://slumber.lucaspickering.me/user_guide/import.html)
- [Build requests dynamically from other requests, files, and shell commands](https://slumber.lucaspickering.me/user_guide/templates/index.html)
- [Browse response data using JSONPath selectors](https://slumber.lucaspickering.me/user_guide/tui/filter_query.html)
- Switch between different environments easily using [profiles](https://slumber.lucaspickering.me/api/request_collection/profile.html)
- And more!

## Examples

Slumber is based around **collections**. A collection is a group of request **recipes**, which are templates for the requests you want to run. A simple collection could be:

```yaml
# slumber.yml
requests:
  get:
    method: GET
    url: https://shoal.lucaspickering.me/fish

  post:
    method: POST
    url: https://shoal.lucaspickering.me/fish
    body:
      type: json
      data:
        {
          "name": "Barry Bartlett",
          "species": "Barracuda",
          "age": 3,
          "weight_kg": 6.2,
        }
```

Create this file, then run the TUI with `slumber`.

For a more extensive example, see [the docs](https://slumber.lucaspickering.me/getting_started.html).

## Private configuration (this fork)

Copy `slumber.example.yml` to `slumber.yml` for a complete localhost-only
crypto example. The sample contains public test keys and invalid personal
identifiers; never use those keys in production. Root `slumber.yml` and
`config.yml`, `private/`, `.env*` (except `.env.example`), `*.local.yml` and
`*.private.yml` are ignored. Keep real endpoints, site names, credentials,
keys and exported responses in those local files, not in tracked examples.
The root sample reads its JSON payload from `examples/native-crypto/`.

Install the local commit guard (preserves existing Git LFS hooks):

```sh
cp .githooks/pre-commit .git/hooks/pre-commit
chmod +x .git/hooks/pre-commit
python scripts/check_sensitive.py --self-test
python scripts/check_sensitive.py
```

The guard checks **staged content**, private file paths, known private labels,
common access-token formats and private keys; CI repeats it. It is a denylist,
not proof that arbitrary data is safe. Review every diff before committing.
Ignoring a file does not remove it from old commits or previously published
archives. Rotate any exposed real credentials and coordinate history cleanup
and replacement of old release assets separately.

## Development

If you want to contribute to Slumber, see `CONTRIBUTING.md` for guidelines, development instructions, etc.
