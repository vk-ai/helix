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
  atlas/pins.json
  chronicle/log.jsonl
```

Vault material belongs in the OS keychain, not in this tree.

## Learning

No LoRA. Everyday improvement is retrieval of playbooks, tool notes, and
preferences written after a clear user verdict. Apprentice (sandbox
propose/execute/select) is optional and not on the default path.

## Slices

1. Home + daemon + CLI + Ollama + memory dirs (this repository)
2. Reliquary + Ask UI
3. Wasm Hands + Switch
4. First adapter
5. Browser pane with human take-over
