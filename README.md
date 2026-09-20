# Helix

**A local-first personal agent OS.**

Helix runs on your machine. A frozen language model plans; tools run only with
capabilities you issued; secrets stay in the OS keychain; memory is files you
can open, edit, and delete. The model never sees API keys or passwords.

> Status: **early slice**. Daemon, CLI, charter packs, Ollama ask path, episode
> write (`--accept` / `--edit` / `--reject`), preferences (`helix pref`),
> Reliquary catalog (`helix secrets`), Ask protocol (`helix grant`),
> shrink-only capability tokens (`helix token`), and Switch egress
> (Loom dials only through the charter allowlist) work today. Wasm Hands
> and connectors are next.

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
| Sole egress | Switch is the only outbound path. Hearthside may dial Ollama loopback only. |

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
  reliquary/               # named secret references (catalog + sealed store)
  atlas/pins.json
  chronicle/log.jsonl
```

Secret *values* belong in the sealed Reliquary store (or OS keychain when the
real backend lands). The model never sees them. Do not paste API keys into chat.

### 4. Start the daemon

```bash
helixd
```

By default it binds **loopback only**: `127.0.0.1:7420`.

Check it:

```bash
helix status
```

You should see the home path, charter pack, Switch summary, and whether Ollama
is reachable.

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

### 7. Reliquary (secret references)

Named secrets live under `~/Helix/reliquary/`. Values are sealed locally; the
CLI never prints them. The OS keychain backend is a stub in this slice
(unwrap is no-op; a local sealed copy is kept).

```bash
# Prefer stdin so the value stays out of shell history:
echo -n 'sk-...' | helix secrets add openai-key
# or:
helix secrets add openai-key --value 'sk-...'
helix secrets list
helix secrets revoke openai-key
```

`list` shows only name, backend, ref id, and created time. Adapters will unwrap
at a Switch boundary later; the model prompt never receives secret values.

### 8. Ask protocol (capability grants)

When the active charter has `writes_require_ask` (all packs today), adapters
must obtain a user grant before performing a write. Grants live in helixd for
the process lifetime:

```bash
# Simulate an adapter requesting permission (also used in tests):
helix grant request plot.write "Write notes.md in the default plot"
helix grant list
helix grant allow-once g-20260918T...
# or: helix grant allow-task <id>  |  helix grant deny <id>
```

`allow-once` is consumed after a single successful use; `allow-task` lasts until
daemon restart. Decisions are appended to `chronicle/log.jsonl`.

### 9. Capability tokens (shrink-only)

helixd issues HMAC-signed tokens capped by the active charter. A holder may only
*attenuate* (drop rights); widening is rejected. Tokens use an ephemeral session
key and become invalid when the daemon restarts.

```bash
# Full hearthside set (plot.read, plot.write, local.model):
helix token issue
# Subset:
helix token issue --rights plot.read,local.model --ttl 3600 > /tmp/t.json
helix token show @/tmp/t.json
helix token verify @/tmp/t.json --require plot.read
# Shrink further (cannot add shell or network):
helix token attenuate @/tmp/t.json --keep plot.read > /tmp/t2.json
```

Rights: `local.model`, `plot.read`, `plot.write`, `network.adapter`, `shell`,
`cloud.model`. Future Hands/adapters will carry these tokens; the model never
forges them.

### 10. Switch (egress)

All Loom HTTP goes through Switch. Under **hearthside**, only loopback Ollama
URLs are allowed. Setting `HELIX_OLLAMA` to a remote host is denied. Desk and
workshop flags for cloud/network do not open arbitrary hosts until an allow
entry is seeded for a real adapter.

```bash
helix status   # includes a switch=… summary line from helixd
```

---

## Platform notes

### macOS

- Reliquary keychain backend is a stub today; local sealed store is used.
  Real Keychain access will prompt later — never paste keys into chat.
- Seatbelt profiles for native adapters land with the Hands runtime.

### Linux

- Build tools: `build-essential` (Debian/Ubuntu) or `gcc` + `make` (Fedora).
- Future Reliquary keychain backend will use libsecret (`libsecret-1-dev`).
- Do not run `helixd` as root.

### Windows

- Use Rustup + MSVC Build Tools.
- The daemon binds IPv4 loopback. Keep Windows Firewall on; do not expose 7420.
- Future Reliquary keychain backend will use DPAPI + Windows Hello.

---

## Configuration

File: `~/Helix/config/charter.toml` (created by `helix init`).

Environment variables override the file:

| Variable | Default | Meaning |
| --- | --- | --- |
| `HELIX_HOME` | `~/Helix` | Data directory |
| `HELIX_BIND` | `127.0.0.1:7420` | Daemon listen address |
| `HELIX_MODEL` | `llama3.2` | Ollama model tag |
| `HELIX_OLLAMA` | `http://127.0.0.1:11434` | Ollama base URL (must pass Switch) |
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
- `helix ask` against Ollama (via Switch)
- Episode write path: `helix ask "…" --accept` / `--reject` / `--edit "…"`
- Optional playbook promotion after two similar successes
- Preference CLI: `helix pref add|list|delete` and capped injection into ask
- Reliquary catalog: `helix secrets list|add|revoke` (local sealed store +
  keychain stub; values never printed or put in model context)
- Ask protocol: `helix grant list|request|allow-once|allow-task|deny` and
  helixd `/v1/grants` endpoints; `writes_require_ask` enforced for adapters
- Capability tokens: `helix token issue|attenuate|verify|show` (shrink-only;
  charter-capped rights)
- Switch egress: Loom checked against charter allowlist before any dial
- Memory directories for episodes / playbooks / notes / prefs
- Chronicle log file created

**Not in this slice** (designed, not shipped)

- Real OS-keychain unwrap (macOS / DPAPI / libsecret)
- Wasm Hands / Wasmtime tools
- Desktop app (Tauri)
- Mail / calendar / browser adapters
- Human take-over for CAPTCHA (browser pane)
- Seeded remote hosts for cloud Loom

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
    helix-reliquary/  named secret references + sealed store
    helix-cap/        shrink-only capability tokens
    helix-switch/     sole egress allowlist (Switch)
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
