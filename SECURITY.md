# Security Model

ARES is a supervisor that observes. It does not act without authorization.
Each guarantee below is marked with whether it is enforced and tested today
or is still only a design target.

## Guarantees

1. **Read-only by default.** Inspection never writes to user files.
   *Enforced today.* ARES currently writes nothing at all: there is no
   database yet. Tests confirm a full `ares repos` run leaves repositories
   byte-for-byte unchanged, including the Git index.
2. **No automatic mutation.** ARES never deletes, moves, renames,
   overwrites, commits, or pushes anything without explicit authorization.
   *Enforced today by absence:* no code path performs such an action.
3. **No AI-generated command execution.** Model output is text for a human
   to read, never executed. *Not applicable yet:* there is no AI layer.
4. **Git is limited to a fixed set of read-only queries.** Callers cannot
   pass arbitrary Git arguments. They choose from a closed list of queries,
   each with a reviewed argument list. Every call also disables optional
   index locks (so `git status` does not rewrite the index), disables
   `core.fsmonitor`, disables prompts, and has a timeout. ARES never runs
   `git fetch`. *Enforced today and tested.*
5. **Secrets stay local.** *Partly enforced:* ARES reads no file contents
   and sends nothing anywhere. *Not yet implemented:* credential redaction
   in logs and reports. Git error text is printed as received, so treat
   output as potentially sensitive until redaction exists.
6. **Minimal cloud exposure.** *Not applicable yet:* no cloud backend
   exists. When one is added it must be off by default with an inspectable
   payload.
7. **Untrusted text is data.** Directory names and Git error text can be
   attacker-controlled. Control characters in them are replaced before
   printing, so a crafted name cannot inject terminal escape sequences.
   *Enforced today and tested.* Prompt-injection handling will matter once
   an AI layer exists.

## Other properties

- Symlinks are never followed during discovery.
- No network access of any kind.
- No dependencies beyond the Rust standard library.
- ARES scans only the folders you give it.

## Reporting a vulnerability

Please open a private security advisory on the GitHub repository rather
than a public issue.
