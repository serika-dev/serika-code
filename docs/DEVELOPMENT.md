# Development

```bash
cargo test -p serika-runtime
cargo test -p serika-cli
cargo test -p serika-desktop
```

`serika-desktop` tests cover `--version`. `--smoke` opens a real window and is not part of `cargo test`.

## What the tests cover

Runtime unit tests cover config validation, the usage JSON from `/api/chat/usage`, permission decisions, unified diffs and exact replace, path traversal, gitignore and sensitive filenames, session save/reload, the `serika-tool` parser, SSE framing, command classification, and a real short process.

`edits_a_file_reviews_the_diff_and_resumes` starts a loopback server that speaks the chat SSE format, runs the agent against a temp project, checks the file change and the stored diff, then resumes the same session. That server is not Serika. The test name does not claim a live account was billed.

CLI tests run the built `serika` binary for `--version`, `--help`, `sessions list`, and `config set` under `SERIKA_HOME`.

## Live checks

`serika doctor` and `serika models` hit the configured API. `serika usage` and a real task require `serika auth login`. There is no flag that marks a mock reply as a successful Serika call.

## Layout

```text
crates/serika-runtime   agent, Serika client, tools, sessions
crates/serika-cli       serika binary
crates/serika-desktop   serika-desktop binary
docs                    behavior of this tree
```

Do not add a second copy of the tool loop inside the CLI or the UI. Both should call `Runtime::run_task`.

## Logging

Logs go to the path in `docs/CONFIGURATION.md`. Level comes from `log_level`. Do not log tokens. The redactor is a backstop for error strings, not a place to dump responses.
