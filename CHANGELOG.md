# Changelog

All notable changes to Serika Code are documented in this file.

## [0.1.0] - 2026-10-30

### Added
- **Shared Agent Runtime (`serika-runtime`)**:
  - Live Serika Accounts authentication (password, 2FA, email magic link, token refresh, and OS keychain storage via Windows Credential Manager, macOS Keychain, and Linux Secret Service).
  - Live `POST /api/chat/message` SSE streaming client, model catalog discovery (`GET /api/chat/models`), and usage tracking (`GET /api/chat/usage`).
  - Workspace-bounded tools (`read_file`, `write_file`, `apply_patch`, `search_files`, `list_files`, `run_command`, `git_status`, `git_diff`, `git_commit`) with four permission modes (`read_only`, `standard`, `restricted`, `full`).
  - Full Git & Version Control engine (`gitinfo::snapshot`, staging/unstaging, discard, branch checkout/creation, isolated Git worktrees, commit log, and push/pull).
  - Self-updater (`updater`) with GitHub Releases inspection, semantic version comparison, and FIPS 180-4 SHA-256 verification.
  - System Setup installer (`setup`) configuring user `PATH`, Start Menu / Desktop shortcuts, macOS `.app` bundle, Linux `.desktop` launcher, and Windows Add/Remove Programs uninstall registration.
- **CLI (`serika`)**:
  - Interactive REPL (`serika chat`), one-shot tasks (`serika run`), `serika auth`, `serika models`, `serika usage`, `serika review`, `serika git`, `serika update`, `serika setup`, `serika desktop`, and `serika doctor`.
- **Desktop (`serika-desktop`)**:
  - T3 Code-inspired carbon-obsidian & rose-violet interface with 60+ FPS non-blocking streaming, pre-parsed markdown/code segments, inbox-style thread sidebar, scratch threads (`Turn into Project…`), one-click isolated Git worktrees (`⑂ Isolate Worktree`), Unified & Split Side-by-Side diff inspector, full Source Control panel, Command Palette (`Ctrl+K` / `⌘+K`), and one-click self-updater.
- **CI / Release Automation**:
  - Multi-OS Blacksmith CI and official release workflows (`blacksmith-4vcpu-ubuntu-2404`, `blacksmith-4vcpu-windows-2025`, `blacksmith-6vcpu-macos-latest`) plus `install.sh` and `install.ps1` installers.
