# Security

The agent can read the project and run programs. Those are separate controls from a sandbox. This release does not isolate processes with Linux namespaces, seccomp, a Windows restricted token, or the macOS sandbox. Do not describe it as a sandbox. Cancellation kills the process tree (`taskkill /T` on Windows, and process-group `kill -TERM -- -<pgid>` on Linux and macOS). That is cleanup, not confinement.

## Permission modes

| Mode | Reads | Writes inside the project | Dev commands (`cargo test`, `npm test`, …) | Other commands | Deletes, commits, pushes, resets, writes outside the project |
| --- | --- | --- | --- | --- | --- |
| `read_only` | yes | no | no | no | no |
| `standard` | yes | yes | yes | asks | asks, and `--yes` does not auto-approve these |
| `restricted` | yes | asks | asks | asks | asks |
| `full` | yes | yes | yes | yes | still asks for destructive ones |

`serika --yes` approves ordinary prompts. It does not approve destructive ones. With no terminal, a prompt is denied and the tool returns that to the model.

Destructive commands are detected by a substring check (`git push`, `git reset`, `git clean`, `rm -rf`, `Remove-Item`, `format`, and similar). A quoted or encoded command can evade the list. That is why it is an approval heuristic, not a sandbox. `read_only` still refuses the command.

## Files

Paths are lexically normalized (so `..` cannot escape) and the nearest existing ancestor is canonicalized so a symlink cannot point outside the project. Tools cannot follow a path outside the project root. Search uses `.gitignore` and the configured exclusion names, and it skips hidden files.

These names are treated as sensitive and always require approval, including in `full`: `.env` and `.env.*`, `.npmrc`, `.pypirc`, `credentials.json`, `secrets.json`, private key filenames, and `*.pem`, `*.pfx`, `*.p12`, `*.key`, `*.kdbx`. `read_only` refuses them.

Directory deletes are refused. `git_commit` only stages the paths it is given, and it always asks. It does not push.

## Commands

Commands run in the project directory through `cmd /C` on Windows and `sh -c` elsewhere, with a timeout and a captured stdout/stderr cap. The GUI sets `CREATE_NO_WINDOW` so a console does not flash. Child environments do not receive a Serika token; the client never puts the token in the environment.

A dev command such as `npm test` can run package scripts. Use `restricted` if that should ask every time.

## Credentials

See `docs/AUTHENTICATION.md`. Logs must not contain passwords, access tokens, or refresh tokens. A redactor replaces JWT-shaped and `sk-` / `ghp_` / `gho_` tokens if one appears in an error string. Request and response bodies are not written to the log.

## Network

Generation goes to the configured Serika API as the signed-in user. The client does not add a second provider when a quota is exhausted.
