# Security

Orbit runs on your Mac with your own keys: Linear (OAuth or an API key),
GitHub through `gh`, optional Anthropic and incident.io keys. Keys live in the
macOS Keychain, are never logged and never leave your machine except to their
own service. There is no Orbit server.

What it guards:

- **Approving and merging** only on your click, bound to the commit you saw,
  checked again right before (`src/github.rs`).
- **Linear comments that command a bot** are only ever posted by your click,
  exactly as configured.
- **Agent sessions** run with your Claude Code or Codex permissions, each in
  its own worktree.

## Reporting a vulnerability

Please do not open a public issue. Use GitHub's
[private vulnerability reporting](https://github.com/ArzelaAscoIi/orbit/security/advisories/new)
with what you found and how to reproduce it. You get an answer within a week.
