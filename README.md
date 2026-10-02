# Helix

**A local-first personal agent OS.**

Helix runs on your machine. A frozen language model plans; tools run only with
capabilities you issued; secrets stay in the OS keychain; memory is files you
can open, edit, and delete. The model never sees API keys or passwords.

> Status: **early slice**. Daemon, CLI, charter packs, Loom, memory write,
> preferences, Reliquary, Ask grants, tokens, Switch, Hands, Atlas, plot
> commits, plot files, mail, desk page, and browser profile pane work today.
> Workshop Apprentice is optional and off the default path (workshop pack only).

[Architecture](docs/ARCHITECTURE.md) · [Threat model](docs/THREAT-MODEL.md) ·
[Install](#install) · [Security](SECURITY.md) · [Contributing](CONTRIBUTING.md)

Clone and build from source (`cargo install --path crates/helix-cli` and
`crates/helixd`). Run `helix init`, start `helixd` on loopback, then `helix status`.

Workshop Apprentice (workshop pack only; sandbox tools; no Reliquary):

```bash
helix charter set workshop
helix apprentice propose "count words in a note"
helix apprentice execute count_words --input "one two three"
helix apprentice select count_words
helix apprentice status
```

Notes land in `~/Helix/memory/tools/`. Hearthside and desk refuse the command.

Browser adapter (desk+ packs only; no CAPTCHA solver):

```bash
helix charter set desk
helix browser open "https://example.com/notes"
helix browser open "https://example.com/login?captcha=1"
helix browser resume
helix browser status
helix browser pane
```

Profile files live under `~/Helix/browser/profile`. The pane is
`http://127.0.0.1:7420/browser`. Dual-licensed MIT OR Apache-2.0.
