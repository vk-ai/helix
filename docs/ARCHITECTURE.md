# Helix architecture

Helix is a capability operating system for one person's agents. The language
model is frozen. Authority is issued, never assumed.

## Processes

- **helixd** — privileged daemon on loopback. Owns home layout, charter, chronicle, and Loom calls.
- **helix** — CLI. Talks only to `helixd`.
- **Hands** (later) — Wasmtime components with deny-by-default WASI.
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

## Learning

No LoRA. Everyday improvement is retrieval of playbooks, tool notes, and
preferences written after a clear user verdict. Apprentice (sandbox
propose/execute/select) is optional and not on the default path.

## Slices

1. Home + daemon + CLI + Ollama + memory dirs (shipped)
2. Reliquary catalog + secrets CLI (shipped; keychain unwrap still stub)
3. Ask protocol: pending grants + `helix grant` + `writes_require_ask` (shipped)
4. Biscuit capability tokens
5. Wasm Hands + Switch
6. First adapter
7. Browser pane with human take-over
