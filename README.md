# Serika Code

Serika Code is a native coding agent for the Serika ecosystem. One Rust runtime drives a CLI (`serika`) and a desktop app (`serika-desktop`). Both sign in with a Serika account, call SerikaAI through the existing chat API, and stay inside that account's usage limits.

It can read a project, edit several files, run build and test commands, show diffs, and keep the session locally. It is not a chatbot with a terminal bolted on, and it is not a website in a window.

## What it does

- Streams replies from Serika (`POST /api/chat/message` on `https://api.serika.dev`).
- Signs in through Serika Accounts (password or email link, refresh, logout).
- Shows usage from `GET /api/chat/usage` — only the fields the server sends.
- Reads, searches, and edits the project, with path checks and a permission mode.
- Runs commands, with timeouts, streamed output, and approval for destructive ones.
- Stores sessions in SQLite and credentials in the OS credential store.

## Platforms

Serika Code runs natively on **Windows**, **Linux** (X11 and Wayland), and **macOS** (Apple Silicon and Intel):

- **Credentials**: Windows Credential Manager (`windows`), Secret Service / D-Bus keyring (`linux`), and macOS Keychain Services (`macos`).
- **Config & Data**: `%APPDATA%\serika-code` / `%LOCALAPPDATA%\serika-code` on Windows, `$XDG_CONFIG_HOME/serika-code` / `$XDG_DATA_HOME/serika-code` on Linux, and `~/Library/Application Support/serika-code` on macOS.
- **Desktop (`serika-desktop`)**: Native window and OpenGL (`glow`) renderer via `eframe`/`winit` across Win32, X11, Wayland, and Cocoa/AppKit.
- **CLI (`serika`)**: Headless native binary with no GUI dependency.

## Quick Install

### Linux & macOS (Bash)

```bash
curl -fsSL https://raw.githubusercontent.com/serika-dev/serika-code/main/install.sh | bash
```

### Windows (PowerShell)

```powershell
irm https://raw.githubusercontent.com/serika-dev/serika-code/main/install.ps1 | iex
```

### Local Setup, Self-Update & Build

See [docs/BUILDING.md](docs/BUILDING.md).

```bash
cargo build --release
cargo test --workspace

# Register binaries in user PATH and create desktop/application shortcuts:
serika setup

# Check for or install the latest release with SHA-256 verification:
serika update --check
serika update
```

## Use

```bash
serika auth login
serika doctor
serika models
serika usage
serika git status
serika desktop
serika "Explain how this project is built"
```

Command details: [docs/CLI.md](docs/CLI.md). Desktop: [docs/DESKTOP.md](docs/DESKTOP.md).

## Limits you should know

Serika's chat API meters each call and streams text. It does not return tool calls to the client. Local tools use a `serika-tool` block that this program parses and runs. That is documented in [docs/SERIKA_API.md](docs/SERIKA_API.md), including the routes that were not used because they do not apply the chat allowance.

Permission checks are not a sandbox. See [docs/SECURITY.md](docs/SECURITY.md).

The desktop UI is egui in a native window, not Qt. Qt was the preferred GUI and is not what shipped, because this tree is built without a Qt SDK. The reason is in [docs/ADR.md](docs/ADR.md).

## License

Apache-2.0. See [LICENSE](LICENSE). The implementation is original. Public coding-agent products were read for interaction ideas and were not copied.

## Docs

- [Architecture](docs/ARCHITECTURE.md)
- [Building](docs/BUILDING.md)
- [Configuration](docs/CONFIGURATION.md)
- [Authentication](docs/AUTHENTICATION.md)
- [Serika API](docs/SERIKA_API.md)
- [Security](docs/SECURITY.md)
- [CLI](docs/CLI.md)
- [Desktop](docs/DESKTOP.md)
- [Development](docs/DEVELOPMENT.md)
