# Praxis

**English** · [한국어](README.ko.md)

**A desktop IDE for running several AI coding agents at once, then reviewing and approving what they changed.**

You can already hand work to an agent from a terminal. Praxis takes over from there. It gives each
task its own **isolated git worktree**, keeps the agent's edits inside it, shows you the diff when the
agent finishes, and **leaves the decision to approve or discard with you**. Your original branch does
not change until you approve.

<!-- Screenshot slot: one home screen + one review/diff screen. Not captured yet. -->

```
prompt  →  isolated worktree  →  agent runs  →  review diff  →  approve or discard
           praxis/<task> branch                  (partial apply supported)
```

## Why Praxis

- **Your original branch stays untouched.** Every task gets a worktree and a dedicated branch under
  `.praxis/worktrees/`. Discarding removes the whole worktree; commits and merges happen only on approval.
- **Run many tasks at once.** Eight by default (configurable from 1 to 64), each in its own worktree
  and terminal session.
- **Mix vendors.** Pick Claude Code, Codex, or Antigravity (Gemini) per task. Send the same prompt to
  several agents to compare results, or have them review each other's work.
- **See what matters before you approve.** The review bar shows protected paths, in-progress git
  operations, expected conflicts, and verification results. Apply only the **hunks you choose**; if
  applying fails, Praxis rolls back to a checkpoint.
- **Pick up where your terminal left off.** Adopt a Claude Code session that is already running and
  continue it as a new task without re-explaining the context. Opening the same session in two places
  at once is refused.
- **Everything runs locally.** The app, repositories, memory files, and index database all live on
  your machine. No server account is required.

## Features

The full list is in the [feature catalog](docs/guide/features.md) (Korean).

| Area | Features |
|---|---|
| Task orchestration | Worktree isolation · concurrent runs · vendor choice · direct-run mode · terminal session adoption |
| Conversation | Prompt queue · side questions alongside the main thread · checkpoint rewind · agent follow-up questions |
| Review and approval | Diff viewer · per-hunk apply · line comments · cross-vendor review · approval readiness · conflict resolution |
| Verification | Build/test gates · goal contracts (protected paths enforced) · decision ledger (optional) |
| Code | Monaco editor · go to definition/references · Quick Open · parquet tables · IPython console · preview window the agent can drive |
| Knowledge | File-based memory injection · Wiki (2D/3D graph · link evidence · ranked search) · code wiki generation · review quizzes while you wait |
| Agent environment | Vendor-neutral `/skills` · MCP · LSP bridge · prompt interview |
| Mobile and more | Mobile PWA with Web Push · voice input · 25 themes |

## Installation

### Prerequisites

| Requirement | How to check |
|---|---|
| git and a locally cloned repository | `git --version` |
| At least one agent CLI | `claude --version` · `codex --version` · `agy --version` |

Each agent CLI must already be logged in or have its API key configured. **If it does not work in your
terminal, it will not work in Praxis either.** Praxis does not install the CLIs for you.

### macOS (Apple Silicon)

1. Download `Praxis_<version>_aarch64.dmg` from the [releases page](https://github.com/JaehaSS/praxis/releases/latest).
2. Open the dmg and drag `Praxis.app` into `Applications`.
3. The first time, do not double-click. **Right-click → Open**, then click **Open** again in the warning dialog.

Step 3 is needed because this build has no Apple code signature or notarization. From the command
line, `xattr -dr com.apple.quarantine /Applications/Praxis.app` does the same thing. You can verify the
download with `shasum -a 256` against the `SHA256SUMS.txt` attached to the release.

### Build from source

Build it yourself on other platforms, or if you would rather avoid the signing warning. In addition to
the prerequisites above, you need:

| Requirement | Notes |
|---|---|
| Node.js 22 or 24 | Tests may fail on 25 |
| Rust stable + Clippy | |
| [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) | Per-OS system libraries |

```sh
npm ci
npm run tauri build
```

On macOS this produces `macos/Praxis.app` and `dmg/` under `src-tauri/target/release/bundle/`.

## Your first task

The [user guide](docs/guide/README.md) walks you through it. The guides are currently written in Korean.

- [Getting started](docs/guide/getting-started.md) — from prerequisites to approving your first task
- [Feature catalog](docs/guide/features.md) — what Praxis can do
- [Workflows & FAQ](docs/guide/workflows-faq.md) — common patterns and troubleshooting

## Platform support

| Item | Status |
|---|---|
| macOS (Apple Silicon) | Where Praxis is developed, built, and used. Distributed as a dmg on the releases page |
| macOS (Intel) | No prebuilt binary. Build from source |
| Windows · Linux | Platform branches and procedure docs exist, but **there is no record of a build or verification** |
| Where tasks run | Locally only. The remote Linux Runner path was removed on 2026-09-19 |
| Auto-update | None. Download new versions from the releases page |

Known limitations are collected under ["범위 밖" (out of scope)](docs/guide/features.md#범위-밖--주의할-것) in the feature catalog.

## Development

`npm run tauri dev` is the only development entry point. There is no separate script that serves the
frontend on its own.

There is no CI; run checks locally.

```sh
npm run check       # frontend (tsc · vitest) + Rust tests + clippy
```

## Further reading

[User guide](docs/guide/README.md) (Korean) · [docs/architecture.md](docs/architecture.md) ·
[DESIGN.md](DESIGN.md) (design system) · [THIRD-PARTY-ASSETS.md](THIRD-PARTY-ASSETS.md)

Design notes, decision records, and the work ledger are not published in this repository. Where the
documents here referred to them, the title is kept without a link.

## License

[MIT](LICENSE). Sources and copyright notices for the file-type icons are in [THIRD-PARTY-ASSETS.md](THIRD-PARTY-ASSETS.md).
