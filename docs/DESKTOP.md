# Desktop

Binary: `serika-desktop`.

It is an egui application in a native window (eframe, glow / OpenGL). It is not a web view. It links `serika-runtime` and uses the same config, credential store, sessions, and agent loop as `serika`.

```text
serika-desktop            open the window
serika-desktop --smoke    open and close, for a startup check
serika-desktop --version
```

## Layout

- Left: project path, open-folder dialog, new session, sessions for this project, account and settings.
- Center: the conversation, streaming text, approval buttons, the input box. Ctrl+Enter sends. Stop cancels the task.
- Right: activity (tool lines and usage), a diff viewer for edits made in the session, and git status / diff.

The diff view is drawn by the UI from the unified diff the runtime stored. It does not embed a browser or a code editor.

## Account

The Account window signs in with a password (and an optional second factor), requests an email link, redeems the link token, and signs out. Usage text is the `/api/chat/usage` summary. If a figure is absent from the response, it is not shown.

## Settings

Model, permission mode, command timeout, session retention, and theme. Model and permission apply to the next task. Timeout and retention are read from the config file at startup, so they apply after a restart. Settings are the same `config.json` the CLI uses.

## What it does not do

It does not open an embedded terminal emulator. Command output is the activity log. It does not sandbox processes. See `docs/SECURITY.md`.
