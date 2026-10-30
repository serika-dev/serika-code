# CLI

Binary name: `serika`.

```text
serika                         interactive session in the current directory
serika "Fix the failing test" one task, then exit
serika run "Fix the failing test"
serika chat                    interactive session
serika auth login              password or email-link sign-in
serika auth logout
serika auth status
serika models
serika usage
serika config show
serika config path
serika config set model deepseek/deepseek-v4.1-flash
serika doctor
serika resume                  latest session in this project
serika resume <id>
serika sessions list
serika sessions rename <id> <title>
serika sessions delete <id>
serika review                  git status and diff, no model call
serika review --ask            read-only review by the agent
serika git status              structured branch, ahead/behind, and working tree status
serika git branches            list local branches
serika git checkout <branch>   switch branch (or create with -b)
serika git stage <path|--all>  stage file(s)
serika git unstage <path|--all>
serika git commit -m "<msg>"   commit staged changes (or --all)
serika git worktree <branch>   create an isolated git worktree for a branch
serika git push / pull         sync with remote
serika update [--check]        check for or install latest GitHub release
serika setup [--uninstall]     install binaries to PATH and create system shortcuts
serika desktop                 launch the native desktop app
```

Global flags, placed before the command:

- `--project <dir>`
- `--model <id>`
- `--permission read_only|standard|restricted|full`
- `--session <id>`
- `--yes` — approve ordinary prompts; destructive actions still ask

In the interactive prompt, `/exit`, `/help`, `/usage`, `/models`, and `/sessions` are local commands. Ctrl+C cancels the current task.

Tool calls print as a dim `tool <name>` line. File edits print a unified diff. Command output is streamed. Approvals are `[y/N]`.

## Exit codes

| Code | Meaning |
| --- | --- |
| 0 | Finished, or a doctor report with no failures |
| 1 | Error, or the agent hit the tool-round cap |
| 2 | Usage limit from Serika |
| 3 | Authentication required or failed |
| 4 | Cancelled |
| 5 | IP rate limit (not a usage-quota body) |

`serika auth status` exits 3 when nobody is signed in.

The CLI does not need a desktop session. Piped use needs a task argument; an empty non-interactive run exits with an error instead of waiting for a prompt.
