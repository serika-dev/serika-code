# Configuration

The CLI and the desktop app read the same files.

## Locations

| Kind | Default | Override |
| --- | --- | --- |
| Config | `%APPDATA%\serika-code\config.json` (Windows), `$XDG_CONFIG_HOME/serika-code/config.json` (Linux), `~/Library/Application Support/serika-code/config.json` (macOS) | `SERIKA_HOME/config.json` |
| Sessions | `%LOCALAPPDATA%\serika-code\sessions.db` (Windows), `$XDG_DATA_HOME/serika-code/sessions.db` (Linux), `~/Library/Application Support/serika-code/sessions.db` (macOS) | `SERIKA_HOME/data/sessions.db` |
| Logs | `%LOCALAPPDATA%\serika-code\logs\serika-code.log` (Windows), `$XDG_DATA_HOME/serika-code/logs/serika-code.log` (Linux), `~/Library/Application Support/serika-code/logs/serika-code.log` (macOS) | `SERIKA_HOME/logs` |
| Cache | `%LOCALAPPDATA%\serika-code` (Windows), `$XDG_CACHE_HOME/serika-code` (Linux), `~/Library/Caches/serika-code` (macOS) | `SERIKA_HOME/cache` |
| Credentials | Windows Credential Manager, Linux Secret Service, or macOS Keychain. Service `serika-code`, account `session` | none |

`SERIKA_HOME` is for tests and portable copies. It is not required.

`serika config path` prints the config file. `serika config show` prints the JSON. `serika config set <key> <value>` writes one field.

An example with no secrets is `config.example.json`.

## Fields

| Key | Default | Meaning |
| --- | --- | --- |
| `api_base_url` | `https://api.serika.dev` | Serika API origin. `http://127.0.0.1` and `http://localhost` are allowed for tests. |
| `accounts_base_url` | `https://accounts.serika.dev` | Serika Accounts origin. |
| `model` | unset | Model id from `GET /api/chat/models`. Unset means pick a coding-oriented model that is actually in the free catalog. |
| `permission` | `standard` | `read_only`, `standard`, `restricted`, or `full`. |
| `command_timeout_secs` | `120` | Ceiling for one command. |
| `max_tool_rounds` | `12` | Model/tool rounds in one task. |
| `session_retention_days` | `90` | Sessions older than this are deleted on startup. `0` keeps them. |
| `log_level` | `info` | `error`, `warn`, `info`, `debug`, or `trace`. |
| `context_exclusions` | `node_modules`, `target`, `dist`, `build`, `.git`, and similar | Directory names skipped by search and the project brief. |
| `max_read_bytes` | `200000` | Cap for a single file read sent to the model. |
| `theme` | `dark` | `dark` or `light`. Used by the desktop app. |
| `max_context_chars` | `60000` | Approximate character budget for prior turns. |
| `stream` | `true` | Reserved. Chat calls stream. |

Environment overrides, also used by tests:

- `SERIKA_API_BASE_URL`
- `SERIKA_ACCOUNTS_BASE_URL`

## Project settings

A project may contain `.serika/project.json`:

```json
{
  "permission": "restricted",
  "extra_exclusions": ["generated"]
}
```

`permission` replaces the global mode when the command line does not pass `--permission`. `extra_exclusions` are added to the global list. Do not put tokens in this file.

## What is not stored in config

Passwords, access tokens, and refresh tokens are not written to the config file, the session database, or logs. Logout deletes the credential-store entry.
