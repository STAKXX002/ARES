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
| `ares github [--online] [--root PATH]... [PATH]...` | Show which repositories have a github.com `origin`, as `owner/name`. By default this reads local git configuration only. With `--online` it also reports open pull requests, open issues, the latest CI run on the default branch, and the last push, using your `gh` login. |
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

[ui]
voice = "plain"
```

Unknown keys are errors, so typos are caught rather than ignored. The
parser supports only the small TOML subset shown above.

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
workflow run). Requirements: `gh` installed and logged in (`gh auth login`).
ARES never sees or stores your token. It only asks `gh` to make the calls.

Nothing goes online unless you pass `--online`. Only repositories whose
`origin` is on github.com are queried. Pull requests are counted from the
first page of 100, so a repository with more shows `100+`. GitHub Enterprise
hosts are not supported.

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
  - [ ] Compare local and remote state (unpushed, behind, diverged)
  - [ ] Recent commits and stale-repository detection
  - [ ] Notifications
- [ ] Phase 3: AI layer (local Ollama, optional Groq)
- [ ] Phase 4: background service and incremental monitoring
- [ ] Phase 5: advanced intelligence

## Known limitations

- Bare repositories are not detected.
- Paths are decoded as UTF-8 lossily, so names with invalid bytes show
  replacement characters.
- CI status is the latest run on the default branch only, and open issues
  are unknown (`?`) when a repository has 100 or more open pull requests.
- GitHub request limits apply. A run over many repositories uses three
  requests per repository.
- `changes` compares only the two most recent scans. It reports nothing
  about repositories that stayed the same, so it cannot yet say how long a
  project has been inactive.
- Paths are stored lossily as UTF-8 in the database as well.
- Only Linux has been tested.

## License

MIT. See [LICENSE](LICENSE).
