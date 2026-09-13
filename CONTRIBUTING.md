# Contributing to Helix

Thank you for helping. Read [docs/THREAT-MODEL.md](docs/THREAT-MODEL.md) first.

## Ground rules

- Do not add ambient shell, host `$HOME` mounts, or `.env` secret loading.
- Do not add LoRA / fine-tuning to the default product path.
- Do not add a CAPTCHA solver.
- New network or filesystem power needs a Charter flag and an Ask path.
- Keep `helixd` small. Prefer a new crate over growing the daemon.

## Setup

```bash
git clone https://github.com/vk-ai/helix.git
cd helix
cargo test --workspace
cargo fmt
cargo clippy --workspace --all-targets -- -D warnings
```

## Pull requests

1. Open an issue for design changes (new adapters, Charter semantics).
2. One concern per PR.
3. Update docs when you change user-visible behaviour.
4. Dual license: MIT OR Apache-2.0. By contributing you agree to both.

## Code of conduct

See [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).
