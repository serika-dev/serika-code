# Desktop

Binary: `serika-desktop`.

It is an egui application in a native window (eframe, glow / OpenGL). It is not a web view. It links `serika-runtime` and uses the same config, credential store, sessions, and agent loop as `serika`.

```text
serika-desktop              open the window
serika-desktop --smoke      open and close, for a startup check
serika-desktop --setup      install binaries to PATH and create system shortcuts
serika-desktop --uninstall  remove installed binaries and shortcuts
serika-desktop --version
```

## Layout & T3 Code Experience

- **Top Command Header**: Workspace switcher, **Scratch Workspace** indicator with one-click **Turn into Project…**, active Git branch pill (`⎇ branch`), one-click **⑂ Isolate Worktree** (spawns a dedicated Git branch and worktree for the current thread), **Update Badge**, **Command Palette (`Ctrl+K` / `⌘+K`)**, and **Inspector Toggle (`Ctrl+B` / `⌘+B`)**.
- **Left Thread Inbox**: `+ New Thread`, `✦ Scratch` thread creation, instant thread search filter, recent workspaces strip, thread inbox with active status indicators, and quick access to Settings, Account, and System Setup.
- **Center Thread Canvas & Floating Composer**: Pre-parsed markdown & syntax-styled code cards with one-click copy, live tool execution pills, destructive-action approval cards, and a floating composer with inline Model & Permission selectors (`Ctrl+Enter` / `⌘+Enter` sends).
- **Right Source Control, Diff & Activity Inspector**:
  - **Source Control**: Branch switcher, new branch & isolated worktree creation, Pull/Push/Refresh, commit composer (`Commit Staged` / `Stage All & Commit`), per-file Stage (`+`), Unstage (`−`), Discard (`↺`), and diff inspection, plus recent commit history.
  - **Diffs**: Multi-scope diff viewer (`Turn Edits`, `Unstaged`, `Staged`) with toggleable **Unified** and **Split (Side-by-Side)** views.
  - **Activity**: Live tool telemetry and account usage summary.

## Account

The Account window signs in with a password (and an optional second factor), requests an email link, redeems the link token, and signs out. Usage text is the `/api/chat/usage` summary. If a figure is absent from the response, it is not shown.

## Settings

Model, permission mode, command timeout, session retention, and theme. Model and permission apply to the next task. Timeout and retention are read from the config file at startup, so they apply after a restart. Settings are the same `config.json` the CLI uses.

## What it does not do

It does not open an embedded terminal emulator. Command output is the activity log. It does not sandbox processes. See `docs/SECURITY.md`.
