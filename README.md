# neondeck

A terminal fleet report for every repo on the grid.

## Why this exists

The current project fleet already covers agent frameworks, music software, games, privacy, sync, mobile, and archival tooling. `neondeck` fills a narrower gap: a small local-first utility that can be audited in one sitting and composed with OpenShark, OpenShield, shell scripts, or other agents.

## v0 scope

- No network access.
- No external Rust dependencies.
- Deterministic output where the filesystem allows it.
- Plain text formats that can be reviewed in Git.
- Real unit tests, not placeholder stubs.

## Install

```sh
cargo build --release
# binary lands at target/release/neondeck
```

## Commands

Scan a directory and print a terminal table, newest commits first:

```sh
neondeck scan ~/Projects/active
```

```
REPO          BRANCH  COMMIT      DIRTY  AHEAD/UP   LANGS                          LAST COMMIT
-----------------------------------------------------------------------------------------------
neondeck      master  2026-09-04  +2     -          Rust:1.3k Markdown:82 TOML:9   Initial scaffold
openshield    main    2026-08-29  +7     +0/-0      Rust:58.0k Markdown:3.7k ...   rebrand: OpenShield with Blackshield identity
Open-Amp      master  2026-08-29  +481   +1/-0      C++:83.0k Markdown:8.9k ...    feat: Blackshield as default UI theme
```

Include TODO/FIXME/HACK marker counts:

```sh
neondeck scan ~/Projects/active --todos
```

Pipe-friendly versioned records for scripting (one repo per line,
`neondeck/v1|name|path|branch|epoch|date|dirty|dirty_files|ahead|behind|langs|todos|fixmes|hacks|subject`):

```sh
neondeck scan ~/Projects/active --format lines
```

Write a Markdown fleet report:

```sh
neondeck report ~/Projects/active --out FLEET_STATUS.md
```

## What's reported

- Repo name, path, current branch (detached HEADs shown as `(detached <sha>)`)
- Last commit date and subject
- Dirty/clean status with changed-file count
- Ahead/behind counts vs upstream (when a tracking branch exists)
- Line counts by language (file-extension heuristic; `target/`, `node_modules/`, `.git/` and friends are excluded)
- Optional TODO/FIXME/HACK counts via `--todos`

## Architecture

`src/main.rs` contains the complete v0 implementation: parsing, validation, pure core functions, CLI dispatch, and unit tests. Git data comes from the `git` CLI (always present on the grid); its output is parsed by pure, unit-tested functions. The next extraction boundary is a `core` module once the format stabilizes; until then, keeping the tape on one reel makes audits cheap.

## Exit codes

- `0` — success
- `1` — runtime error (path missing, not a directory, write failure)
- `2` — usage error (missing or unknown arguments)

## Roadmap

- [x] Repo discovery and fleet table
- [x] Markdown fleet report
- [x] Versioned line format for scripting
- [ ] Watch mode
- [ ] OpenShark fleet adapter

## Development

```sh
cargo fmt --check
cargo test
cargo run -- --help
```

## Safety

Local commits only. Never push or create remotes without explicit instruction. Do not weaken validation to make a failing test pass.

---
Made by [synth](https://github.com/synthalorian) with synthclaw 🎹🦞
