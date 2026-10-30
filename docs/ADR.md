# ADR 001 — Native Rust runtime, CLI, and desktop

## Status

Accepted.

## Context

Serika Code needs one agent runtime shared by a command-line tool and a desktop application. Both must talk to the existing Serika accounts service and the Serika chat API, stream responses, run tools against a local project, and stay small and fast.

The preferred desktop stack was Qt 6 with C++. This machine had Git, Python, and Node, and did not have a C++ compiler, CMake, Ninja, or a Qt SDK. Installing the Visual Studio Build Tools and Qt is a multi-gigabyte toolchain and was not required to meet the product constraints: a compiled native program, no Electron, no webview as the interface, low idle cost, and a single implementation of the agent.

Rust 1.99 (GNU host) was installed instead. C dependencies (SQLite) are compiled with LLVM MinGW. The desktop UI uses egui through eframe's glow backend: a native window and an OpenGL renderer, not a browser.

## Decision

- One Cargo workspace.
- `serika-runtime` owns configuration, authentication, the Serika client, sessions, tools, permissions, and the agent loop.
- `serika` is the CLI binary.
- `serika-desktop` is the egui application.
- Local data is SQLite. Secrets go to the operating system credential store.
- Model traffic uses `POST /api/chat/message` on `https://api.serika.dev` with `temporary: true`, so the server's usage meter applies and the user's chat.serika.dev history is not filled with tool traffic.
- Local tools use a fenced `serika-tool` block. The chat API does not return provider tool calls to the client. See `docs/SERIKA_API.md`.

## Consequences

- CLI and desktop cannot drift apart on auth, quotas, or tool policy.
- Startup is a single native process. There is no bundled browser.
- The text tool protocol depends on the model following instructions. It is not the provider's native tool channel, and the documentation says so.
- A future Qt shell can call the same runtime if a C ABI is added later. That is not in this release.
- Windows release builds need a C compiler for the bundled SQLite. `scripts/dev-env.ps1` records the toolchain used to develop this tree.
