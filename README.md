# Helix

**A local-first personal agent OS.**

Helix runs on your machine. A frozen language model plans; tools run only with
capabilities you issued; secrets stay in the OS keychain; memory is files you
can open, edit, and delete. The model never sees API keys or passwords.

> Status: **early slice**. Daemon, CLI, charter packs, Loom, memory, Reliquary,
> Ask grants, tokens, Switch, Hands, Atlas, plot commits, plot files, mail,
> desk page, and browser profile pane (`helix browser`, `/browser`) work today.
> Workshop Apprentice is next (optional, never default).

[Architecture](docs/ARCHITECTURE.md) · [Threat model](docs/THREAT-MODEL.md) ·
[Install](#install) · [Security](SECURITY.md) · [Contributing](CONTRIBUTING.md)

See the repository files for install and command details. Browser adapter
(desk+):

```bash
helix charter set desk
helix browser open "https://example.com/notes"
helix browser open "https://example.com/login?captcha=1"
helix browser resume
helix browser status
helix browser pane
```

Challenges pause. A human solves them. There is no CAPTCHA solver in core.
