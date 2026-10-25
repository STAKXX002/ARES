# Security Model

ARES is a supervisor that observes. It does not act without authorization.
Each guarantee below is marked with whether it is enforced and tested today
or is still only a design target.

## Guarantees

1. **Read-only by default.** Inspection never writes to user files.
   *Enforced today.* `repos`, `status`, `changes`, and `config` write
   nothing to user files. (If an older ARES database needs a schema upgrade,
   opening it applies that to ARES's own database only.)
   `scan` writes only to ARES's own database (see below). Tests confirm
   that full runs of `repos` and `scan` leave repositories byte-for-byte
   unchanged, including the Git index, and that `repos`, `status`, and
   `changes` never create a database and `changes` leaves it byte-for-byte
   unchanged.
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
   and sends nothing anywhere except the repository names in `--online`
   requests (see below). Remote URLs often embed access tokens, so
   they are reduced to `owner/name` the moment they are parsed, and
   non-GitHub remotes are reported without their URL. Tests check that a
   token in an `origin` URL never reaches any output. Text from `gh` is redacted. *Not yet
   implemented:* the same redaction for git's own error text, which is
   printed as received.
6. **Minimal cloud exposure.** *Not applicable yet:* no cloud backend
   exists. When one is added it must be off by default with an inspectable
   payload.
7. **Untrusted text is data.** Directory names and Git error text can be
   attacker-controlled. Control characters in them are replaced before
   printing, so a crafted name cannot inject terminal escape sequences.
   *Enforced today and tested.* Prompt-injection handling will matter once
   an AI layer exists.

## GitHub access

`ares github --online` is the only part of ARES that uses the network, and
only when you pass `--online`. It does so by running your own `gh`.

- **ARES never handles a token.** It does not read, store, or print one.
- **Only fixed read requests can be made.** Callers choose from three
  request kinds. Each is `gh api --method GET --hostname github.com` plus a
  path built from a validated owner, repository name, and (optionally)
  branch, and always beginning with `repos/`. There is no way to add a
  request body, field flags, another HTTP method, or another endpoint.
  Tests check every call `gh` receives.
- **Your login is broader than ARES needs.** A normal `gh` login has scopes
  such as `repo` and `workflow`, which could modify things. ARES cannot be
  stopped by GitHub from misbehaving with that token, so its safety here
  rests on the closed request list above. For server-side enforcement, run
  with a read-only fine-grained token in `GH_TOKEN` (see the README).
- **Output is bounded and checked.** `gh` runs with a timeout in its own
  process group, with its output size capped. JSON is parsed with a depth
  limit, types are checked, and strings are length-limited. Text that came
  from the network is stripped of control characters before printing.
- **Credentials are redacted.** Anything `gh` writes to its error output
  passes through redaction (GitHub tokens, URL passwords, authorization
  headers) before it can be shown. Debug logging variables (`GH_DEBUG`,
  `DEBUG`) are removed from the environment `gh` runs in.
- **Only the repository name goes out.** The requests contain `owner/name`
  of repositories whose `origin` is on github.com. No file contents, local
  paths, or commit data are sent.

## The database

`ares scan` records snapshots in `$XDG_DATA_HOME/ares/ares.db` (or
`~/.local/share/ares/ares.db`). It holds repository paths and Git state
(branch, upstream, change counts) and the text of any Git error for a
repository that could not be read. It holds no file contents. The directory
is created 0700 and the file is set to 0600, both covered by tests. The
schema is versioned, and a database written by a newer ARES is refused
instead of modified. Only the newest 1000 scans are kept.

## Other properties

- Symlinks are never followed during discovery.
- No network access of any kind.
- Three direct dependencies (`rusqlite` with bundled SQLite, `serde_json`,
  and `libc`) plus their transitive crates, pinned in `Cargo.lock`. Review
  lockfile changes like any other code.
- Child processes (`git`, `gh`) run with no stdin, a timeout, a cap on
  captured output, and in their own process group, which is killed as a
  whole on timeout.
- ARES scans only the folders you give it.

## Reporting a vulnerability

Please open a private security advisory on the GitHub repository rather
than a public issue.
