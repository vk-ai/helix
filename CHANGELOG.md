# Changelog

All notable changes to Helix are documented here. Format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Helix versions follow [SemVer](https://semver.org/).

## [Unreleased]

### Added

- Reliquary catalog (`helix-reliquary` crate): named secret references under
  `~/Helix/reliquary/` with local sealed store (`catalog.json` + `sealed.json`)
- OS keychain backend stub (macOS Keychain / DPAPI / libsecret) — unwrap is
  no-op; local sealed copy kept for this slice
- CLI: `helix secrets list|add|revoke` (list never prints values; add accepts
  `--value` or stdin)
- Preference files: `helix pref add|list|delete`; rules live under
  `~/Helix/memory/prefs/<name>.md`
- Prefs are injected into the ask prompt with a 1500-character soft cap
  (`PREFS_CONTEXT_CHAR_CAP`); keyword retrieval no longer double-counts prefs
- Memory write path: `helix ask --accept`, `--reject`, and `--edit TEXT` write
  episode JSON under `~/Helix/memory/episodes/`
- Optional playbook promotion after two similar accepted/edited successes
  (`memory/playbooks/`)

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
