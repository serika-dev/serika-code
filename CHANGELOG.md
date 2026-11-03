# Changelog

All notable changes to Serika Code are documented in this file.

## [Unreleased]

### Added
- **Installers**: a Windows setup (per-user, no admin needed; Start menu, optional desktop
  shortcut and `serika` on PATH; clean uninstall), a Linux AppImage with delta updates and a
  `.deb` for Debian and Ubuntu. Every release builds them.
- **Replies type out** at an even pace instead of landing in bursts, and new paragraphs fade in.
- **Long messages** fold to a preview with "Show all N lines".
- **Stash conflicts you can work through**: popping or applying a stash that clashes opens a
  card with each place side by side (keep yours, keep the stash, or both), whole-file choices,
  "Ask Serika" to resolve it for you, Undo, and Finish. A popped stash is only removed once
  you finish. Merges, rebases and cherry-picks that stop on conflicts use the same card.
- **Your own changes never block a stash**: they are set aside, the stash goes in, and they
  come back on top (conflicts, if any, go to the same card).

### Changed
- Stash rows have labelled **Apply** and **Pop** buttons instead of copy and download icons.

### Fixed
- Pasted text with `<...>` (an email, a log) showed as an empty bubble.
- Opening a long message or clicking in a chat no longer jumps to the bottom.
- The Windows desktop package includes the C++ runtime, so it starts without Qt installed.

## [0.2.0] - 2026-11-03

The Qt desktop app grows into a full workspace.

### Added
- **Chats that keep running**: switch chat or project and a running chat carries on in the
  background; the sidebar shows running, waiting-for-approval and finished chats, with stop
  from the sidebar.
- **Drafts**: a new chat shows in the sidebar at once; unsent text is kept per chat.
- **Git panel**: stashes found by ticket number with per-file diffs and apply/pop/drop;
  incoming and outgoing commits; working changes; history; fetch, pull, push and publish with
  visible progress; branch search, switch and create.
- **Git accounts**: commit identity and sign-in login per project, GitHub logins imported from
  the GitHub CLI and Git Credential Manager (with profile pictures); network commands sign in
  as the project's login and fall back to your other logins.
- **Project settings**: overview, Git, Agent (instruction files, skills, MCP) and danger zone.
- **Project instructions**: `AGENTS.md`, `CLAUDE.md` and `.serika/instructions.md` are read
  by Serika's agent.
- **MCP**: the 2026-07-28 revision plus every earlier one over stdio, Streamable HTTP and
  HTTP+SSE; servers imported from Claude Code, Claude Desktop, Codex and `.mcp.json`;
  Serika's browser served to Claude Code and Codex.
- **Skills** from the project, Serika, Claude Code (and plugins) and Codex.
- **Terminal panel** and **docks**: right and bottom docks with draggable tabs, resizable in
  width and height.
- **Browser**: live screencast, real mouse input, clipboard, favicons, reliable tabs.
- **Sidebar**: group by project, date or one list; sort by activity, newest or name;
  `Ctrl+1…9` and `Ctrl+Tab` to move between chats.
- Richer Markdown: headings, nested and task lists, callouts, tables sized to their content.

### Changed
- Much faster: model and account lists answer from saved copies and refresh in the
  background; git overview uses fewer git calls; GitHub CLI answers are cached.
- Smoother motion: springy dialogs, animated sidebar rows, smooth wheel scrolling everywhere.

### Fixed
- Tab switching races in the browser and docks, lost layout writes, CLIs installed after start
  not being found, removing the current project, and many smaller issues.

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
