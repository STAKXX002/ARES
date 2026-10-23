# ARES

**Autonomous Runtime and Environment Supervisor**

A lightweight personal development environment supervisor for Ubuntu.
ARES watches your local projects and Git repositories, tracks GitHub state,
and tells you what needs attention.

> Core principle: ARES observes deterministically, reasons selectively,
> and acts only with authorization.

## Status

Early development. Nothing is usable yet. See the roadmap below.

## Goals

- Systems utility first, AI assistant second.
- Routine monitoring never needs an LLM.
- Useful with no AI, no internet, and no GitHub connection.
- Low resource use, measured and reported rather than assumed.
- Read-only by default. See [SECURITY.md](SECURITY.md).

## Roadmap

- [ ] Phase 1: core, CLI, repo discovery, git status, SQLite, file index
- [ ] Phase 2: GitHub awareness (issues, PRs, workflow status)
- [ ] Phase 3: AI layer (local Ollama, optional Groq)
- [ ] Phase 4: background service and incremental monitoring
- [ ] Phase 5: advanced intelligence

## Planned CLI

```bash
ares status
ares scan
ares repos --dirty
ares report
```

## License

MIT. See [LICENSE](LICENSE).
