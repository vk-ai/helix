# Security policy

## Supported versions

Only `main` is supported during 0.x.

## Report a vulnerability

Do **not** open a public issue for a vulnerability.

Email **vinaykm.mails@gmail.com** with:

- affected commit or tag
- impact (cap escape, vault leak, bind beyond loopback, unsigned tool as trusted)
- reproduction on an account you own

We will acknowledge within 7 days and aim to ship a fix before any disclosure.

## Safe research

- Test only on your machine and your Helix home.
- Do not scan other people's daemons.
- Do not store captured secrets in tickets.

See [docs/THREAT-MODEL.md](docs/THREAT-MODEL.md).
