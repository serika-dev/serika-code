# Architecture

Serika Code is three crates and one process model.

```text
serika  ──┐
          ├── serika-runtime ── HTTPS ── api.serika.dev
serika-desktop ┘                 └── HTTPS ── accounts.serika.dev
                 │
                 ├── OS credential store
                 └── sessions.db (SQLite)
```

The UI sends a `TaskRequest` and reads `AgentEvent` values. It does not call the model or touch the filesystem itself. Approvals go through `ApprovalHub`: the runtime emits `ApprovalRequired` and waits; the CLI or the desktop resolves the id.

## Runtime

| Module | Job |
| --- | --- |
| `config`, `paths` | Shared JSON config and directories |
| `credentials` | OS keychain, or an in-memory store in tests |
| `client` | Accounts login/refresh/logout and chat streaming |
| `model` | Catalog and usage types matched to the live JSON |
| `sessions` | SQLite sessions, messages, tool rows, diffs |
| `context` | Project brief and history trimming |
| `protocol` | `serika-tool` fences |
| `tools`, `workspace`, `patch`, `shell`, `gitinfo` | Local actions |
| `permissions` | The four modes |
| `agent` | The loop: prompt, stream, tools, repeat, save |
| `doctor` | Install and connectivity checks |

## Loop

1. Resolve the project, permission, and model.
2. Create or resume a session and store the user message.
3. Build a project brief (tree, languages, manifests, a short README excerpt). The full tree is not sent.
4. `POST /api/chat/message` with `temporary: true` and stream the text.
5. If the reply contains `serika-tool` blocks, check permission, run the tools one at a time, store the results, and go back to step 4.
6. If the reply has no tool block, the task is finished. The summary is the model's text, not a claim invented by the host.
7. If the round cap is hit, the host says the task stopped unfinished.

Cancellation is a `CancellationToken` checked between tools and during the HTTP read and the child process.

## Editing

`replace_in_file` replaces one exact span (or all spans when `all` is true) and refuses an ambiguous match. `apply_patch` applies a unified diff and refuses a hunk that does not match the file. Both record a unified diff on the session. `write_file` is for new files or a deliberate full rewrite, and it is capped at 1 MB.

## Index

There is no vector database. Search is a gitignore-aware walk. The brief is cached for the task. Search tools walk again so they see files the task just wrote.

## Events

`Started`, `Status`, `MessageChunk`, `ReasoningChunk`, `MessageCompleted`, `ToolRequested`, `ApprovalRequired`, `ToolStarted`, `ToolOutput`, `ToolFinished`, `FileChanged`, `UsageUpdated`, `Completed`, `Failed`, `Cancelled`.

## Why the desktop is not Qt

`docs/ADR.md`. The window is native. The renderer is OpenGL through egui. Agent code is not in the widgets.
