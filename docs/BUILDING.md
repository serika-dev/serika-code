# Building

Serika Code is a Cargo workspace. Release binaries are `serika` and `serika-desktop`.

## Dependencies

- Rust 1.88 or newer. 1.99 GNU was used for this tree on Windows.
- A C compiler and archiver, because SQLite is compiled into the binary (`rusqlite` `bundled`).
  - Windows with the MSVC Rust toolchain: Visual Studio Build Tools (the `cl` compiler).
  - Windows with the GNU Rust toolchain: LLVM MinGW (`MartinStorsjo.LLVM-MinGW.UCRT`). Dot-source `scripts/dev-env.ps1` before running `cargo`: it points `CC_x86_64_pc_windows_gnu` and `AR_x86_64_pc_windows_gnu` at LLVM MinGW's `clang` and `llvm-ar`, keeps Rust's bundled linker off LLVM MinGW's `PATH`, and wraps Rust's bundled GNU `dlltool` with LLVM MinGW's assembler so import libraries match GNU `ld`.
  - Linux (Debian / Ubuntu):
    ```bash
    sudo apt-get install build-essential pkg-config libssl-dev \
      libxkbcommon-x11-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
      libwayland-dev libgl1-mesa-dev libegl1-mesa-dev
    ./scripts/build-unix.sh
    ```
  - macOS (Apple Silicon & Intel):
    ```bash
    xcode-select --install
    ./scripts/build-unix.sh
    ```
  - The HTTP client uses the platform's native TLS stack (`native-tls`: SChannel on Windows, OpenSSL on Linux, Security.framework on macOS). Credentials use Windows Credential Manager, Linux Secret Service (`org.freedesktop.secrets`), and macOS Keychain Services.
- Git, for the agent's git tools. The programs themselves build without it.
- A desktop session with OpenGL, for `serika-desktop`. The CLI does not need a GUI.

## Commands

```bash
cargo test --workspace
cargo build --release
```

The binaries are:

- `target/release/serika` (or `serika.exe`)
- `target/release/serika-desktop` (or `serika-desktop.exe`)

Copy them somewhere on `PATH` if you want to run `serika` from any directory.

## First run

```bash
serika doctor
serika auth login
serika
```

`serika doctor` checks the config file, the local database, model connectivity, and, when you are signed in, the usage endpoint. It does not print tokens.

## Tests that do not call Serika

`cargo test` uses local fixtures and a loopback HTTP server. It does not sign in to a real account and it does not spend usage. A live check is `serika models` or `serika doctor` after you authenticate.
