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
   to read, never executed, and no model has any tool access. *Enforced today
   by absence:* no code path runs, opens, or interprets anything a model says.
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
   token in an `origin` URL never reaches any output. Error text from `gh`
   and `git` is redacted before it can be shown.
6. **Minimal cloud exposure.** *Enforced today:* no cloud backend exists.
   The only model backend is Ollama, and its client refuses any address that
   is not loopback before it connects. When a cloud backend is added it must
   be off by default, opt-in per run, with aliasing and an inspectable payload.
7. **Untrusted text is data.** Directory names and Git error text can be
   attacker-controlled. Control characters in them are replaced before
   printing, so a crafted name cannot inject terminal escape sequences.
   *Enforced today and tested.* Text a model returns gets the same treatment.
   See the AI layer below for prompt injection.

## GitHub access

`ares github --online` is the only part of ARES that uses the network, and
only when you pass `--online`. It does so by running your own `gh`.

- **ARES never handles a token.** It does not read, store, or print one.
- **Only fixed read requests can be made.** Callers choose from four
  request kinds (repository details, open pull requests, latest workflow
  run, and a branch comparison). Each is `gh api --method GET --hostname github.com` plus a
  path built from a validated owner, repository name, branch, and (for the
  comparison) a full-length lowercase commit id, and always beginning with
  `repos/`. There is no way to add a
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
- **Little goes out.** The requests contain `owner/name` of repositories
  whose `origin` is on github.com, the default branch name GitHub reported,
  and the commit id your clone last fetched for that branch. No file
  contents, local paths, or commit messages are sent.

## The AI layer

`ares ask` can use a model running on your own machine through Ollama. It
never uses a cloud service. `ares ask --dry-run` prints the exact payload and
sends nothing.

- **Loopback only.** The Ollama client connects to `127.0.0.1` on the port in
  `[ai] ollama_port`. It refuses any non-loopback address before it opens a
  connection, so no setting can point it at a remote server.
- **Facts, not repositories.** A model only ever receives numbered facts. Each
  is a fixed sentence filled with numbers ARES measured, plus the repository's
  directory name. Git error text, branch names, commit messages, file
  contents, remote URLs, and paths are never included. Tests check this.
- **Structure cannot be broken from outside.** Facts are written one JSON
  object per line, so a hostile directory name (quotes, newlines, text that
  looks like instructions) stays inside its own string. Tests create such
  directories and check that the payload keeps its exact shape.
- **Bounded.** The question is cleaned and length-limited, the payload has a
  size cap that drops the lowest-priority facts first, the request is
  deterministic (temperature 0, fixed seed), the reply length is capped, and
  the wait has a deadline.
- **Inspectable.** One function builds the payload, and `--dry-run` prints its
  exact output. What you see is what the backend is given.
- **Name aliasing.** `--alias` replaces directory names with `R1`, `R2`, ... in
  the payload. Tests check that no real name or path appears in what is sent,
  and that the answer is shown with real names only on your machine.
- **Output is verified, then shown as text.** A reply must be JSON statements
  that cite fact ids. ARES shows a statement only if every cited id exists,
  every number in it appears in the cited facts, and every repository it names
  is the subject of a cited fact. Uncited statements may not contain numbers
  or repository names, and are marked as inferred. Anything else is dropped
  and counted. If nothing verifies, the measured facts are printed instead.
  Control characters are stripped from model text before it is printed.
- **Nothing is written or run.** `ares ask` writes nothing, including to
  ARES's own database, and does not execute model output.

Remaining risks, stated plainly:

- A directory *name* is attacker-influenced text that a model reads (for
  example, `ignore all previous instructions`). The mitigations are escaping,
  the instruction that facts are data, the absence of any tool access, the
  checker, and that model output is never executed. The worst outcome is a
  misleading summary.
- The checks are mechanical. They catch invented numbers, invented citations,
  and claims attached to the wrong repository. They cannot tell whether a
  verified sentence is a *good* reading of its facts, which is why the answer
  carries its citations and a reminder that it is a summary, not advice.
- A model can leave out something important, and the checker cannot notice an
  omission.

Design targets for the parts not built yet: a cloud backend is off by default
and needs explicit consent for each run, with name aliasing on by default and
the key read from the environment and never logged.

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
- The network is used only for `ares github --online` and `ares ask --online`
  (through your own `gh`), and for `ares ask`, which talks to Ollama on
  `127.0.0.1` only.
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
