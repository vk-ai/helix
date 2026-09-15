# Changelog

All notable changes to Helix are documented here. Format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Helix versions follow [SemVer](https://semver.org/).

## [Unreleased]

### Added

- Memory write path: after `helix ask`, CLI flags `--accept` / `--reject` / `--edit TEXT` (or interactive prompt on TTY) write episode JSON under `~/Helix/memory/episodes`
- Optional playbook promotion after two similar accepted successes (keyword overlap)

### Fixed

- CI `cargo fmt --check` on the initial four pushes
- Unused `helix-protocol` dependency on `helix-charter`
- Replace abbreviated LICENSE-APACHE with the full Apache License 2.0 text

## [0.1.0] - 2026-09-13

### Added

- Workspace crates: `helixd`, `helix-cli`, `helix-protocol`, `helix-charter`, `helix-memory`
- `helix init`, `helix status`, `helix ask`, `helix charter`
- Loopback-only HTTP daemon (`127.0.0.1:7420`)
- Charter packs: hearthside, desk, workshop
- Home layout under `~/Helix`
- File-based memory directories (episodes, playbooks, tools, prefs)
- Ollama generate path when a local runtime is available
