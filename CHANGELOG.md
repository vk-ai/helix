# Changelog

All notable changes to Helix are documented here. Format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Helix versions follow [SemVer](https://semver.org/).

## [Unreleased]

### Added

- Atlas pins (`helix-atlas` crate): digest-pinned tools in `~/Helix/atlas/pins.json`
- CLI: `helix atlas list|pin|unpin|verify` — pin by SHA-256 of file bytes
- `helix hands run` classifies modules via Atlas: trusted pins run unrestricted;
  unsigned / drifted tools are **UNTRUSTED** and get a default fuel cap
  (`UNTRUSTED_FUEL_CAP` = 50_000_000 instructions) unless `--fuel` is set
- Wasm Hands host (`helix-hands` crate): Wasmtime + WASI preview1 with
  deny-by-default policy — no network, no ambient env, single preopened FS
  root: the active plot at guest `/plot`
- CLI: `helix hands run <module.wasm> [--plot NAME] [--fuel N] [-- args…]`
- Library API: `helix_hands::run_module` + `HandsConfig::for_plot`
- Loom provider interface (`helix-loom` crate): Ollama + optional OpenAI-compat
  behind Switch; API key from Reliquary reference only (never in model context)
- `HELIX_LOOM`, `HELIX_OPENAI_BASE`, `HELIX_OPENAI_KEY_REF` select cloud path;
  cloud blocked on hearthside (`allow_cloud_model` + Switch allowlist)
- helixd ask path routes all model calls through `helix_loom::complete`
- Switch egress gate (`helix-switch` crate): sole allowlist for outbound HTTP
  used by Loom; Hands/adapters must not raw-dial
- Hearthside allowlist = Ollama loopback only (`127.0.0.1` / `localhost` / `::1`)
- Workshop/desk cloud flags do not open the internet until hosts are seeded
- helixd routes `/api/tags` and `/api/generate` through Switch before dialing
- `Status.switch` summary line; `helix status` surfaces it when present
- Shrink-only capability tokens (`helix-cap` crate): HMAC-SHA256 tokens issued by
  helixd; attenuation may only remove rights (Biscuit-equivalent, no widen)
- helixd: `POST /v1/tokens`, `/v1/tokens/attenuate`, `/v1/tokens/verify`
- CLI: `helix token issue|attenuate|verify|show`
- Rights keyed to charter: `local.model`, `plot.read`, `plot.write`,
  `network.adapter`, `shell`, `cloud.model`
- Session MAC key is ephemeral (tokens invalid after daemon restart)
- Ask protocol: pending capability grants in helixd (`GET/POST /v1/grants`,
  `POST /v1/grants/:id/decide`)
- CLI: `helix grant list|request|allow-once|allow-task|deny`
- `writes_require_ask` enforcement helper for future adapters (Once grants
  are consumed on use; Task grants last until daemon restart)
- Charter show prints `writes_require_ask`
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
