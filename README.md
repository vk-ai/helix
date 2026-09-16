# Helix

**A local-first personal agent OS.**

Helix runs on your machine. A frozen language model plans; tools run only with
capabilities you issued; secrets stay in the OS keychain; memory is files you
can open, edit, and delete. The model never sees API keys or passwords.

> Status: **early slice**. Daemon, CLI, charter packs, Ollama ask path, episode
> write (`--accept` / `--edit` / `--reject`), and preferences (`helix pref`)
> work today. Reliquary, Wasm tools, and connectors are next.

[Architecture](docs/ARCHITECTURE.md) · [Threat model](docs/THREAT-MODEL.md) ·
[Install](#install) · [Security](SECURITY.md) · [Contributing](CONTRIBUTING.md)

---

## What Helix is

| Principle | What that means |
| --- | --- |
| Local first | Default brain is [Ollama](https://ollama.com) (or MLX / llama.cpp). Cloud models are optional and gated. |
| No ambient authority | The model emits *intents*. Doing work requires a capability token that can only shrink. |
| Secrets stay sealed | Reliquary stores references. Unwrap happens per approved call, never in the prompt. |
| Memory you can read | Episodes, playbooks, tool notes, and preferences live under `~/Helix/memory`. |
| Frozen weights | Everyday improvement is retrieval of what worked for *you*. No LoRA in the product path. |

Helix is **not** a chatbot that inherits your login session.

---

## Requirements

| | Minimum |
| --- | --- |
| OS | macOS 13+, Linux (x86_64 or aarch64), Windows 11 |
| Rust | 1.75+ (only if you build from source) |
| Disk | ~200 MB for Helix + space for a local model |
| Optional | [Ollama](https://ollama.com) for a local LLM |

You do **not** need Docker, a GPU, or an API key to use the default **hearthside** pack (chat + plot + local model).

---

## Install

### 1. Install a local model runtime (recommended)

```bash
# macOS / Linux
curl -fsSL https://ollama.com/install.sh | sh
ollama serve
ollama pull llama3.2
```

Windows: install from [ollama.com/download](https://ollama.com/download), then
`ollama pull llama3.2` in PowerShell.

Any Ollama tag works. Set `HELIX_MODEL` if you use another name (see
[Configuration](#configuration)).

### 2. Install Helix

**From source (supported today)**

```bash
git clone https://github.com/vk-ai/helix.git
cd helix
cargo install --path crates/helix-cli
cargo install --path crates/helixd
```

`helix` is the CLI. `helixd` is the daemon.

**Package managers** — Homebrew, winget, and `.deb` / `.rpm` are planned.
Until then, use the source install above.

### 3. Initialize your home

```bash
helix init
```

This creates:

```text
~/Helix/
  config/charter.toml      # active capability pack (default: hearthside)
  plots/default/           # the only workspace tools can see
  memory/
    episodes/
    playbooks/
    tools/
    prefs/
  atlas/pins.json
  chronicle/log.jsonl
```

Secrets are **not** written here. They belong in the OS keychain once Reliquary
ships. Do not put API keys in `~/Helix`.

### 4. Start the daemon

```bash
helixd
```

By default it binds **loopback only**: `127.0.0.1:7420`.

Check it:

```bash
helix status
```

You should see the home path, charter pack, and whether Ollama is reachable.

### 5. Talk to Helix

```bash
helix ask "What can you do in this charter?"
```

If Ollama is running, Helix sends a constrained prompt (charter summary +
retrieved memory + your text) to the local model. If Ollama is down, the CLI
still prints the charter and memory context so you can verify the pipeline.

Record a verdict so Helix can learn from this turn:

```bash
helix ask "What can you do in this charter?" --accept
helix ask "How should I structure notes?" --edit "Prefer short bullet lists."
helix ask "Ignore this" --reject
```

Accepted and edited replies write episode JSON under `~/Helix/memory/episodes/`.
After two similar successes, a short playbook may appear under
`memory/playbooks/`.

### 6. Preferences

Short standing rules live as one file each under `~/Helix/memory/prefs/`:

```bash
helix pref add tone "Prefer concise bullet replies."
helix pref add format "Use ISO dates."
helix pref list
helix pref delete format
```

On every `helix ask`, preferences are injected into the model context first
(soft-capped at 1500 characters) so the model can follow your rules without
fine-tuning.

---

## Platform notes

### macOS

- Keychain access comes later with Reliquary. Grant Helix access when the OS
  prompts; never paste keys into chat.
- Seatbelt profiles for native adapters land with the Hands runtime.

### Linux

- Build tools: `build-essential` (Debian/Ubuntu) or `gcc` + `make` (Fedora).
- Reliquary will use libsecret. Install `libsecret-1-dev` when that slice ships.
- Do not run `helixd` as root.

### Windows

- Use Rustup + MSVC Build Tools.
- The daemon binds IPv4 loopback. Keep Windows Firewall on; do not expose 7420.
- Reliquary will use DPAPI + Windows Hello.

---

## Configuration

File: `~/Helix/config/charter.toml` (created by `helix init`).

Environment variables override the file:

| Variable | Default | Meaning |
| --- | --- | --- |
| `HELIX_HOME` | `~/Helix` | Data directory |
| `HELIX_BIND` | `127.0.0.1:7420` | Daemon listen address |
| `HELIX_MODEL` | `llama3.2` | Ollama model tag |
| `HELIX_OLLAMA` | `http://127.0.0.1:11434` | Ollama base URL |
| `HELIX_PACK` | `hearthside` | Charter pack: `hearthside`, `desk`, `workshop` |

Charter packs live in [`charter-packs/`](charter-packs/):

- **hearthside** — chat, plot, local model. No network adapters.
- **desk** — plus read-only connectors later; writes require Ask.
- **workshop** — plus plot-scoped shell and optional cloud Loom later.

Switch pack:

```bash
helix charter set hearthside
helix charter show
```

---

## Everyday use (what exists vs what is next)

**Works in this slice**

- Local home layout and charter packs
- Daemon on loopback
- `helix ask` against Ollama
- Episode write path: `helix ask "…" --accept` / `--reject` / `--edit "…"`
- Optional playbook promotion after two similar successes
- Preference CLI: `helix pref add|list|delete` and capped injection into ask
- Memory directories for episodes / playbooks / notes / prefs
- Chronicle log file created

**Not in this slice** (designed, not shipped)

- Reliquary UI and OS-keychain unwrap
- Wasm Hands / Wasmtime tools
- Biscuit capability tokens
- Desktop app (Tauri)
- Mail / calendar / browser adapters
- Human take-over for CAPTCHA (browser pane)

Learning stays **file-based**. Helix does not fine-tune the model. After you
accept or edit a result, an episode JSON is written under
`~/Helix/memory/episodes`. You can delete any file to make it forget.

---

## Security expectations

- Bind only to loopback until device pairing exists.
- Treat the model as untrusted. It must not gain filesystem or network power
  from a prompt.
- Do not run unsigned tools as trusted. Atlas pins (digest, not tag) are the
  planned trust root.
- Report vulnerabilities privately — see [SECURITY.md](SECURITY.md).

Read [docs/THREAT-MODEL.md](docs/THREAT-MODEL.md) before connecting any account.

---

## Repository layout

```text
helix/
  crates/
    helixd/           daemon
    helix-cli/        CLI
    helix-protocol/   shared types
    helix-charter/    pack load + summary
    helix-memory/     home layout + retrieval stub
  charter-packs/      hearthside, desk, workshop
  docs/               architecture and threat model
```

---

## Development

```bash
git clone https://github.com/vk-ai/helix.git
cd helix
cargo test --workspace
cargo run -p helixd
cargo run -p helix-cli -- status
```

See [CONTRIBUTING.md](CONTRIBUTING.md).

---

## License

Dual-licensed under [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT), at your
option.

---

## Maintainers

Early work lives at [github.com/vk-ai/helix](https://github.com/vk-ai/helix).
Issues and pull requests are welcome once you have read the threat model.
