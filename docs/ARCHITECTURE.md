# Helix architecture

Helix is a capability operating system for one person's agents. The language
model is frozen. Authority is issued, never assumed.

## Processes

- **helixd** — privileged daemon on loopback. Owns home layout, charter, chronicle, Loom calls, and capability token MAC key.
- **helix** — CLI. Talks only to `helixd`.
- **Switch** — sole egress gate inside helixd (and later Hands). Allowlist is
  derived from the active charter; Loom cannot raw-dial.
- **Hands** — Wasmtime host (`helix-hands`) with deny-by-default WASI; only the
  active plot is preopened (guest `/plot`). CLI: `helix hands run`.
- **File adapter** — plot-scoped list/read/write/delete via `helix files`
  (same plot root as Hands; no host shell).
- **Adapters** (later) — native processes for mail, browser, OS APIs.
- **Desktop** (later) — Tauri app for Ask banners, Reliquary, Plot, Chronicle.

## Data

```text
~/Helix/
  config/charter.toml
  plots/<id>/
  memory/{episodes,playbooks,tools,prefs}/
  reliquary/          # catalog.json + sealed.json (values never in prompts)
  atlas/pins.json
  chronicle/log.jsonl
```

Secret values live in Reliquary (local sealed store today; OS keychain later).
They are references in catalog until unwrap at an adapter/Switch boundary.

## Capability tokens

`helix-cap` issues shrink-only tokens (HMAC-SHA256 over a canonical payload).
Rights are drawn from the active charter and may only be reduced by attenuation.
The daemon holds an ephemeral session key; tokens do not survive restart.
This is the product equivalent of Biscuit attenuation without the full biscuit-auth stack on the default path.

## Switch (egress)

`helix-switch` classifies every outbound URL before Loom (or a future adapter)
may dial:

- **hearthside**: loopback hosts only (`127.0.0.1`, `localhost`, `::1`) for the
  local model runtime.
- **desk / workshop**: same loopback rule; remote hosts remain empty until an
  explicit allow entry is added for a connector or cloud Loom. Charter flags
  alone do not open the internet.

## Loom

`helix-loom` is the sole model provider interface. Completions always pass
Switch before dialing. Providers:

- **Ollama** (default) — loopback `/api/generate` when `allow_local_model`.
- **OpenAI-compat** — optional `/chat/completions`; requires `allow_cloud_model`,
  a Switch-allowed host (`HELIX_OPENAI_BASE`), and a Reliquary secret name
  (`HELIX_OPENAI_KEY_REF`). The API key is unwrapped only inside the provider
  and never enters the prompt. Hearthside keeps cloud blocked.

Env: `HELIX_LOOM=ollama|openai|auto`, `HELIX_OPENAI_BASE`, `HELIX_OPENAI_KEY_REF`.

## Hands (Wasm host)

`helix-hands` runs core WASI preview1 modules under Wasmtime. By default the
guest sees:

- **FS**: only the active plot directory, preopened at `/plot` (read + write).
- **No network**, no ambient environment variables, no host `$HOME`.

Optional fuel limits instruction count. Atlas pins classify tools before run;
plot-scoped file tools share this plot root. Adapters must still go through
Switch for any egress.

## Atlas

`helix-atlas` stores SHA-256 pins in `atlas/pins.json`. Trust is the digest, not
the path. `helix hands run` classifies each module: matching trusted pins run
without an automatic fuel cap; unsigned or drifted tools are marked untrusted
and receive a default instruction budget (`UNTRUSTED_FUEL_CAP`).

## Plot commits

`helix-memory` records content-addressed snapshots of a plot working tree under
`plots/<id>/.helix/commits/<sha256>/`. The id is the SHA-256 of a sorted
manifest (relative path + per-file digest); `.helix/` is excluded. CLI:
`helix plot commit|list|rewind|status`. Rewind restores the tree and updates
`HEAD`.

## Plot file adapter

`helix files` (backed by `HelixHome::plot_*` helpers) lists, reads, writes, and
deletes files strictly under the active plot. Paths cannot escape via `..`,
absolute forms, or into `.helix`. This is the first native tool surface; it
does not invoke a host shell.

## Learning

No LoRA. Everyday improvement is retrieval of playbooks, tool notes, and
preferences written after a clear user verdict. Apprentice (sandbox
propose/execute/select) is optional and not on the default path.

## Slices

1. Home + daemon + CLI + Ollama + memory dirs (shipped)
2. Reliquary catalog + secrets CLI (shipped; keychain unwrap still stub)
3. Ask protocol: pending grants + `helix grant` + `writes_require_ask` (shipped)
4. Capability tokens: issue / attenuate / verify (shipped)
5. Switch egress proxy (shipped)
6. Loom provider interface: Ollama + optional OpenAI-compat (shipped)
7. Wasm Hands host: Wasmtime + plot-scoped FS (shipped)
8. Atlas pins: digest-pinned tools + untrusted fuel cap (shipped)
9. Plot commits: content-addressed snapshot + rewind (shipped)
10. First plot-scoped file adapter through Hands boundary (shipped)
11. Browser pane with human take-over
