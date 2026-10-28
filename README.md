# ARES

**Autonomous Runtime and Environment Supervisor**

A lightweight personal development environment supervisor for Ubuntu.
ARES inspects your local Git repositories and tells you what needs attention.

> Core principle: ARES observes deterministically, reasons selectively,
> and acts only with authorization.

## Status

Early development. Today ARES can discover Git repositories under folders
you choose, report their working-tree state, record scan snapshots in its
own SQLite database, and (only when you ask with `--online`) look up pull
requests, issues, and CI status on GitHub through your existing `gh` login.
Everything else works offline. Only `ares scan` writes anything, and only
to ARES's own database.

Not implemented yet: file indexing, local-versus-remote comparison, GitHub
notifications, any AI backend, background service mode. See the roadmap.

## Build and run

Requires a Rust toolchain (1.75 or newer), `git`, and a C compiler such as
`gcc` (`sudo apt install build-essential`). SQLite is compiled from source as
part of the build, so the first build takes about a minute.

```bash
cargo build --release
./target/release/ares repos ~/code
./target/release/ares repos --dirty ~/code
```

ARES scans nothing until you name a folder, either on the command line or
in the config file.

## Commands

| Command | What it does |
| --- | --- |
| `ares repos [--dirty] [--root PATH]... [PATH]...` | List repositories and their state. `--dirty` shows only those with uncommitted changes (plus any that could not be read). |
| `ares status [--root PATH]... [PATH]...` | Overview of which repositories need attention, grouped by reason: conflicted, uncommitted, unpushed, behind, unreadable. |
| `ares scan [--root PATH]... [PATH]...` | Same report as `status`, and also records a snapshot in ARES's database. Reports how the previous recorded scan compares in size and age. |
| `ares changes` | Compare the last two recorded scans: new and removed repositories, repos that became dirty or clean, unpushed commits that appeared or were pushed, branch switches, and repos that became unreadable. Read-only. |
| `ares github [--online] [--root PATH]... [PATH]...` | Show which repositories have a github.com `origin`, as `owner/name`. By default this reads local git configuration only. With `--online` it also reports open pull requests, open issues, the latest CI run on the default branch, the last push, and whether GitHub has commits you have not fetched, using your `gh` login. |
| `ares stale [--days N] [--root PATH]... [PATH]...` | List repositories whose newest commit (on any local branch) is at least N days old, longest-quiet first, with any uncommitted files or unpushed commits that suggest unfinished work. Default 90 days. Offline and read-only. |
| `ares ask [QUESTION] --dry-run [--online] [--alias] [--root PATH]...` | **In progress.** Shows exactly what an AI model would be sent for your question, and sends nothing. No model backend exists yet, so `--dry-run` is the only mode. See "Asking questions" below. |
| `ares config` | Show the config file location and effective settings. |
| `ares help`, `ares version` | Usage and version. |

Global options: `--config PATH`, `--voice NAME`.

Exit codes: `0` success, `1` runtime or config error, `2` usage error.

## Configuration

Optional. The file is `$XDG_CONFIG_HOME/ares/config.toml`, or
`~/.config/ares/config.toml`. A missing file means defaults.

```toml
[scan]
roots = ["~/code"]            # absolute paths or ~/...
max_depth = 6                 # 0 to 32
exclude_dirs = ["node_modules", "target"]   # replaces the built-in list

[git]
timeout_secs = 10             # 1 to 300
max_parallel = 4              # repositories inspected at once, 1 to 16
                              # default: your CPU thread count, between 2 and 8

[stale]
days = 90                     # quiet threshold for `ares stale`, 1 to 3650

[ai]
model = ""                    # Ollama model name, e.g. "qwen2.5:3b"; empty = none chosen
ollama_port = 11434           # local Ollama server, 1 to 65535 (127.0.0.1 only)
timeout_secs = 120            # how long to wait for an answer, 5 to 900
unload_after = false          # ask Ollama to free the model's memory after each answer

[ui]
voice = "plain"
```

Unknown keys are errors, so typos are caught rather than ignored. The
parser supports only the small TOML subset shown above (strings, integers,
`true`/`false`, and arrays of strings). The `[ai]` settings only describe
your own local Ollama; nothing in them turns on any network use beyond
`127.0.0.1`.

## Voices

Reports can be worded in different styles. Pick one with `--voice` or
`[ui] voice`. The default is `plain`.

| Voice | Style |
| --- | --- |
| `plain` | Neutral, no flavor text. Best for scripts and bug reports. |
| `cheerful` | Warm and encouraging. |
| `deadpan` | Dry and understated. |

A voice may also add a closing remark to `changes`. A voice only chooses
words around the facts. The per-repository lines are
identical for every voice, read errors are always stated plainly, and
tests enforce both rules.

## Asking questions (AI layer, in progress)

ARES is useful without any AI, and always will be. The optional AI layer is
meant to answer questions such as "what needs my attention?" by summarizing
facts ARES has already measured. What exists today is the safe half:

```bash
ares ask "which projects are neglected?" --dry-run --root ~/projects
```

This prints the complete payload a model would receive and sends nothing
anywhere. It is built from numbered **facts**, one fixed sentence each:

```
{"id":"F3","kind":"uncommitted","repo":"StakOS","text":"has 2 files uncommitted (0 staged, 0 unstaged, 2 untracked)."}
{"id":"F4","kind":"quiet","repo":"StakOS","text":"has had no commit for 107 days."}
```

What goes in: counts and ages that ARES read itself (uncommitted files,
unpushed commits, days since the last commit, and with `--online`, GitHub pull
request, issue, CI, and fetch counts), plus the repository's directory name.

What never goes in: file contents, commit messages, branch names, git or `gh`
error text, remote URLs, or paths. Facts are fixed sentences filled with
numbers, so repository-controlled text cannot reach a model except the
directory name, which is JSON-escaped and length-limited.

`--alias` replaces directory names with `R1`, `R2`, ... in the payload and
prints the key separately, so real names can stay on your machine.

Facts are numbered in priority order (conflicts and unreadable repos first), and
if there are too many, the lowest-priority ones are left out and counted.
Planned next: a local model through Ollama, answer checking (the model must cite
fact ids, and ARES verifies them), and an optional cloud backend that is off by
default.

## What "stale" means

`ares stale` goes by the date of the newest commit on any **local branch**,
not by file changes or GitHub activity. A repo is "active" if that commit is
newer than the threshold. Quiet does not mean abandoned: a finished project
is quiet too. The line also shows uncommitted files and unpushed commits,
because a quiet repo with loose changes is the likeliest unfinished work.
Commits that exist only on `origin` (fetched but not merged) do not count.

## What a repository line means

```
StakML   main  12 changed (3 staged, 7 unstaged, 2 untracked), 2 ahead
```

All values come from `git status`, not from inference. "ahead" and
"behind" are relative to the upstream as of the last fetch. ARES never
runs `git fetch`.

## Storage

`ares scan` records each scan in a SQLite database at
`$XDG_DATA_HOME/ares/ares.db`, or `~/.local/share/ares/ares.db`. It stores,
per repository: path, name, branch, upstream, ahead/behind counts, and the
change counts shown by `repos`. It stores no file contents. The directory
is created with mode 0700 and the file with 0600, and the newest 1000 scans
are kept. To remove all history, delete the file.

`repos` and `status` never create or touch the database. `changes` only
reads it, and never creates it.

## GitHub access

`ares github --online` runs `gh api --method GET ...` for a small fixed set
of read-only requests per repository (details, open pull requests, latest
workflow run, and a comparison of GitHub's default branch with your last
fetch). Requirements: `gh` installed and logged in (`gh auth login`).
ARES never sees or stores your token. It only asks `gh` to make the calls.

Nothing goes online unless you pass `--online`. Only repositories whose
`origin` is on github.com are queried. Pull requests are counted from the
first page of 100, so a repository with more shows `100+`. GitHub Enterprise
hosts are not supported.

**Do I need to fetch?** ARES reads where your clone last saw `origin`'s
branches (`git for-each-ref`, local only) and asks GitHub how its default
branch compares with that commit. The phrases you may see:

| Phrase | Meaning |
| --- | --- |
| `origin/main in sync` | Your last fetch is GitHub's current tip. |
| `GitHub has N new commits, fetch needed` | Run `git fetch`. |
| `diverged from GitHub (...)` | Each side has commits the other lacks. |
| `GitHub is N behind your last fetch (history rewritten?)` | The branch was rewound or force-pushed. |
| `your last fetch of main is not on GitHub` | Your `origin/main` commit is unknown to GitHub. |
| `main never fetched` | No local copy of the branch to compare. |

ARES never runs `git fetch` itself.

If you want GitHub itself to enforce read-only access rather than relying on
ARES, run it with a fine-grained read-only token for that one command:

```bash
GH_TOKEN=<fine-grained token: metadata, issues, pull requests, actions: read> \
    ares github --online ~/projects
```

## Dependencies

Three direct dependencies, all pinned in `Cargo.lock`:

| Crate | Why |
| --- | --- |
| `rusqlite` (bundled SQLite) | scan history storage |
| `serde_json` | parsing the JSON that `gh` returns, with size limits and a depth limit |
| `libc` | one call, to kill a whole process group when a child times out |

They bring a small set of transitive crates (24 in total). Everything else,
including the config parser and argument parsing, is plain standard library.

## Safety

See [SECURITY.md](SECURITY.md) for the exact guarantees and which are
enforced today.

## Measuring resource use

```bash
scripts/measure.sh 200        # 200 repos, default parallelism
scripts/measure.sh 200 1      # same, one at a time
```

Creates throwaway repositories, runs `ares repos`, and prints wall time and
peak memory. Numbers depend on your machine, so measure locally. Most of
the time is spent starting one `git` process per repository, which is why
`max_parallel` helps.

## Roadmap

- [ ] Phase 1: core
  - [x] Repo discovery, Git status parsing, read-only runner
  - [x] Config file, CLI, report voices
  - [x] Resource measurement script
  - [x] SQLite storage (scan history)
  - [ ] Basic file indexing
- [ ] Phase 2: GitHub awareness
  - [x] Identify GitHub repositories from local remotes
  - [x] Pull requests, issues, latest CI result, last push (through your `gh` login)
  - [x] Compare your last fetch with GitHub (fetch needed, diverged, rewritten)
  - [x] Stale-repository detection (`ares stale`)
  - [ ] Recent commits
  - [ ] Notifications
- [ ] Phase 3: AI layer (local Ollama, optional Groq)
  - [x] Verified facts with stable ids
  - [x] Exact payload and `ares ask --dry-run`
  - [ ] Local Ollama backend
  - [ ] Checking that answers cite real facts
  - [ ] `ares ask` answering questions
  - [ ] Optional Groq backend (off by default)
- [ ] Phase 4: background service and incremental monitoring
- [ ] Phase 5: advanced intelligence

## Known limitations

- Bare repositories are not detected.
- Paths are decoded as UTF-8 lossily, so names with invalid bytes show
  replacement characters.
- The fetch comparison covers the default branch only, and is only as fresh as
  your last `git fetch`. Branches other than the default are not compared.
- CI status is the latest run on the default branch only, and open issues
  are unknown (`?`) when a repository has 100 or more open pull requests.
- GitHub request limits apply. A run over many repositories uses up to
  four requests per repository.
- `changes` compares only the two most recent scans. It reports nothing
  about repositories that stayed the same, so it cannot yet say how long a
  project has been inactive.
- Paths are stored lossily as UTF-8 in the database as well.
- Only Linux has been tested.

## License

MIT. See [LICENSE](LICENSE).
