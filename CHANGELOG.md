# Changelog

All notable changes to Helix are documented here. Format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Helix versions follow [SemVer](https://semver.org/).

## [Unreleased]

### Added

- Loopback desk page (`helix-desk`): helixd serves `/desk` and `/v1/desk` for
  Ask banners, Reliquary names (never values), and Chronicle tail
- First plot-scoped file adapter: `helix files ls|read|write|delete` operates only
  under the active plot (same root Hands preopens at `/plot`); rejects `..`,
  absolute paths, and `.helix`; no host shell

### Fixed

- CI: `cargo fmt --check` across workspace crates after the desk slice
- Clippy `-D warnings`: needless `return` in `helix-cap`, redundant `matches!` in loom tests
- `plot_resolve` rejects absolute paths (e.g. `/etc/passwd`) before stripping separators

## [0.1.0] - 2026-09-13

### Added

- Workspace crates: `helixd`, `helix-cli`, `helix-protocol`, `helix-charter`, `helix-memory`
- `helix init`, `helix status`, `helix ask`, `helix charter`
- Loopback-only HTTP daemon (`127.0.0.1:7420`)
- Charter packs: hearthside, desk, workshop
- Home layout under `~/Helix`
- File-based memory directories (episodes, playbooks, tools, prefs)
- Ollama generate path when a local runtime is available
