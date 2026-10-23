# Security Model

ARES is a supervisor that observes. It does not act without authorization.
These guarantees are design constraints, and each one is meant to be enforced
by code structure and covered by tests, not only promised here.

## Guarantees

1. **Read-only by default.** Inspection commands never write to user files.
   ARES writes only to its own database and its own config directory.
2. **No automatic mutation.** ARES never deletes, moves, renames, overwrites,
   commits, or pushes anything without explicit user authorization.
3. **No AI-generated command execution.** Model output is text for a human to
   read. It is never passed to a shell or executed.
4. **Git allowlist.** Only read-only git subcommands are invoked. Network
   operations such as `git fetch` are off unless explicitly enabled.
5. **Secrets stay local.** By default files are indexed by metadata and hash
   only. Content is read only for an explicit allowlist (for example README
   and Markdown files). Credentials are redacted from logs and reports.
6. **Minimal cloud exposure.** The cloud AI backend is off by default. When
   enabled, only the minimum required context is sent, and the exact payload
   can be inspected before sending.
7. **Untrusted text is data.** Commit messages, issue titles, and README
   content may be attacker-controlled. They are delimited and treated as data
   when placed in model context.

## Status

Nothing is implemented yet. This document describes the target model, and
each guarantee will be marked as enforced once the code and tests exist.

## Reporting a vulnerability

Please open a private security advisory on the GitHub repository rather than
a public issue.
