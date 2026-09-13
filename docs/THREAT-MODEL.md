# Threat model

## Assets

- OS-keychain secrets (planned)
- Plot files and memory files
- Chronicle (audit integrity)
- Charter (what the agent may attempt)
- Local model transcripts

## Actors

- The user on the same machine
- The frozen language model (treated as hostile / injectable)
- Unsigned tools and web content
- Other processes on the host
- Optional cloud model providers

## Assumptions

- `helixd` binds loopback only.
- The user does not run `helixd` as root.
- Connecting a cloud provider is a Charter decision, not a default.

## Controls (current slice)

- Loopback bind check in `helixd`
- Charter packs that disable cloud, network adapters, and shell by default
- No secrets in `~/Helix`
- Chronicle append for `ask`

## Controls (designed, not shipped)

- Biscuit capabilities that only attenuate
- Reliquary unwrap at the adapter boundary
- Wasm deny-by-default Hands
- Switch allowlists
- Human Ask in the app UI, not in chat

## Out of scope for v0

- Multi-user / multi-tenant
- Remote exposure of the daemon
- Automated CAPTCHA solving
- Weight updates of the model
