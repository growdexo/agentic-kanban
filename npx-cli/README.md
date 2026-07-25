# Agentic Kanban

> Orchestrate Claude Code, Gemini CLI, Codex, Amp and other coding agents from a single kanban board.

## Quick Start

Run Agentic Kanban instantly, without installation:

```bash
npx agentic-kanban
```

This launches the application locally and opens it in your browser.

## What is Agentic Kanban?

Agentic Kanban is a local-first tool for developers who delegate work to AI coding agents. It gives you a kanban board on top of your git repositories so you can plan tasks, run one or more coding agents against them in isolated worktrees, review the diffs, and merge the results — all from one place.

### Key features

- **Multiple agents** — switch between Claude Code, Gemini CLI, Codex, Amp and others.
- **Parallel & sequential execution** — run agents on many tasks at once, or chain them.
- **Isolated worktrees** — every task attempt runs in its own git worktree, so experiments never clobber your working tree.
- **Review & merge** — inspect diffs, start dev servers, and merge successful changes back to your main branch.
- **Task tracking** — follow the status of everything your agents are working on.
- **Centralised MCP config** — manage coding-agent MCP servers in one place.
- **Remote via SSH** — open projects in your local editor when running on a remote server.

## How it works

1. **Add a project** — import an existing git repository or create a new one.
2. **Create tasks** — describe what needs to be built or fixed.
3. **Run an agent** — let a coding agent work on the task in an isolated worktree.
4. **Review** — see exactly what changed with a git diff.
5. **Merge** — incorporate successful changes into your codebase.

## Requirements

- Node.js (for `npx`)
- Git
- Your preferred code editor (optional, for opening worktrees)

## Supported platforms

- Linux x64
- Windows x64
- macOS x64 (Intel)
- macOS ARM64 (Apple Silicon)

## Links

- Source & issues: https://github.com/growdexo/agentic-kanban
- License: Apache-2.0

Agentic Kanban began as a fork of [Vibe Kanban](https://github.com/BloopAI/vibe-kanban) (© Bloop AI, Apache-2.0) and is now developed independently.
