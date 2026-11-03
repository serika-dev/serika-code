mod tui;
mod chat;
mod editor;
mod highlight;
mod ipc;

use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::time::Duration;

use clap::{Parser, Subcommand};
use serika_runtime::{
    AgentEvent, Error, LoginOutcome, PermissionMode, Runtime, TaskRequest,
};
use tokio_util::sync::CancellationToken;
use zeroize::Zeroize;

use crate::tui::{Term, TurnRenderer};

#[derive(Parser)]
#[command(
    name = "serika",
    version,
    about = "Serika Code — fast native coding agent for your terminal and desktop"
)]
struct Cli {
    /// Model id from `serika models`. Defaults to your configured model or `local/workspace`.
    #[arg(long, global = true)]
    model: Option<String>,
    /// read_only, standard, restricted, or full
    #[arg(long, global = true)]
    permission: Option<String>,
    /// Project directory. Defaults to the current directory.
    #[arg(long, global = true)]
    project: Option<PathBuf>,
    /// Approve ordinary prompts. Destructive actions still ask when a terminal is attached.
    #[arg(long, global = true)]
    yes: bool,
    /// Continue this session id.
    #[arg(long, global = true)]
    session: Option<String>,
    /// Plain output without colours or boxes (also when NO_COLOR is set).
    #[arg(long, global = true)]
    no_color: bool,
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Interactive Codex-style session in the current project.
    Chat,
    /// Run one task and exit.
    Run { prompt: Vec<String> },
    /// Sign in, sign out, or show who is signed in.
    Auth {
        #[command(subcommand)]
        action: AuthCommand,
    },
    /// List models discovered from local engines, provider APIs, and Serika Cloud.
    Models,
    /// Show the account usage returned by Serika.
    Usage,
    /// Show or change configuration.
    Config {
        #[command(subcommand)]
        action: ConfigCommand,
    },
    /// Check the install, sign-in, and Serika connectivity.
    Doctor,
    /// Resume a session. With no id, resume the latest one in this project.
    Resume { id: Option<String> },
    /// List, rename, or delete local sessions.
    Sessions {
        #[command(subcommand)]
        action: SessionCommand,
    },
    /// Show the current git diff. `--ask` sends it to the agent in read-only mode.
    Review {
        #[arg(long)]
        ask: bool,
        focus: Vec<String>,
    },
    /// Inspect and manage Git version control, branches, and isolated worktrees.
    Git {
        #[command(subcommand)]
        action: GitCommand,
    },
    /// Check for updates or self-update Serika Code binaries from GitHub releases.
    Update {
        /// Only check if a newer release is available without installing.
        #[arg(long)]
        check: bool,
        /// Force reinstall even if the current version matches.
        #[arg(long)]
        force: bool,
    },
    /// Install Serika Code into the user PATH, create desktop shortcuts, or uninstall.
    Setup {
        /// Custom installation prefix directory.
        #[arg(long)]
        prefix: Option<PathBuf>,
        /// Do not modify the user PATH environment variable.
        #[arg(long)]
        no_path: bool,
        /// Do not create Start Menu / Desktop / Application shortcuts.
        #[arg(long)]
        no_shortcuts: bool,
        /// Uninstall Serika Code binaries and shortcuts.
        #[arg(long)]
        uninstall: bool,
        /// When uninstalling, also remove saved configuration and session database.
        #[arg(long)]
        purge: bool,
    },
    /// Launch the Qt desktop app.
    Desktop,
    /// JSON-lines host used by the Qt desktop. Not a user-facing command.
    Ipc,
    /// A task written directly after `serika`.
    #[command(external_subcommand)]
    Task(Vec<String>),
}

#[derive(Subcommand)]
enum GitCommand {
    /// Show structured repository branch, ahead/behind, and working tree status.
    Status,
    /// List local branches.
    Branches,
    /// Switch to a branch (or create one with `-b`).
    Checkout {
        branch: String,
        #[arg(short = 'b', long)]
        create: bool,
    },
    /// Stage a file or `--all` modified/untracked files.
    Stage {
        path: Option<String>,
        #[arg(long)]
        all: bool,
    },
    /// Unstage a file or `--all` staged files.
    Unstage {
        path: Option<String>,
        #[arg(long)]
        all: bool,
    },
    /// Commit staged changes (or `--all` modified files).
    Commit {
        #[arg(short, long)]
        message: String,
        #[arg(short, long)]
        all: bool,
    },
    /// Create an isolated git worktree for a branch.
    Worktree {
        branch: String,
    },
    /// Push current branch to remote.
    Push,
    /// Pull latest changes (`--ff-only`).
    Pull,
}

#[derive(Subcommand)]
enum AuthCommand {
    Login,
    Logout,
    Status,
}

#[derive(Subcommand)]
enum ConfigCommand {
    Show,
    Path,
    Set { key: String, value: String },
}

#[derive(Subcommand)]
enum SessionCommand {
    List,
    Rename { id: String, title: Vec<String> },
    Delete { id: String },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let runtime = match Runtime::open() {
        Ok(runtime) => runtime,
        Err(err) => exit_error(err),
    };
    let term = Term::detect(cli.no_color);
    let code = match dispatch(runtime, cli, &term).await {
        Ok(code) => code,
        Err(err) => {
            tui::error_line(&term, &format!("serika: {err}"));
            err.exit_code()
        }
    };
    std::process::exit(code);
}

async fn dispatch(runtime: Runtime, cli: Cli, term: &Term) -> serika_runtime::Result<i32> {
    let project = project_path(cli.project)?;
    let permission = match cli.permission {
        Some(value) => Some(
            PermissionMode::parse(&value)
                .ok_or_else(|| Error::msg("permission must be read_only, standard, restricted, or full"))?,
        ),
        None => None,
    };
    match cli.command {
        None => repl(&runtime, &project, cli.model, permission, cli.yes, cli.session, term).await,
        Some(Commands::Chat) => repl(&runtime, &project, cli.model, permission, cli.yes, cli.session, term).await,
        Some(Commands::Run { prompt }) => {
            run_once(&runtime, &project, &prompt.join(" "), cli.model, permission, cli.yes, cli.session, term).await
        }
        Some(Commands::Task(prompt)) => {
            run_once(&runtime, &project, &prompt.join(" "), cli.model, permission, cli.yes, cli.session, term).await
        }
        Some(Commands::Auth { action }) => auth(&runtime, action, term).await,
        Some(Commands::Models) => models(&runtime, term).await,
        Some(Commands::Usage) => usage(&runtime, term).await,
        Some(Commands::Config { action }) => config_cmd(&runtime, action),
        Some(Commands::Doctor) => doctor(&runtime, term).await,
        Some(Commands::Resume { id }) => {
            let id = match id.or(cli.session) {
                Some(id) => id,
                None => runtime
                    .sessions
                    .list(Some(&project))?
                    .into_iter()
                    .next()
                    .map(|session| session.id)
                    .ok_or_else(|| Error::msg("this project has no saved sessions"))?,
            };
            repl(&runtime, &project, cli.model, permission, cli.yes, Some(id), term).await
        }
        Some(Commands::Sessions { action }) => sessions(&runtime, &project, action, term),
        Some(Commands::Review { ask, focus }) => review(&runtime, &project, ask, &focus, cli.model, cli.yes, term).await,
        Some(Commands::Git { action }) => git_cmd(&runtime, &project, action, term).await,
        Some(Commands::Update { check, force }) => update_cmd(check, force, term).await,
        Some(Commands::Setup {
            prefix,
            no_path,
            no_shortcuts,
            uninstall,
            purge,
        }) => setup_cmd(&runtime, prefix, no_path, no_shortcuts, uninstall, purge),
        Some(Commands::Desktop) => launch_desktop(&project, term),
        Some(Commands::Ipc) => ipc::serve(runtime).await,
    }
}

fn project_path(project: Option<PathBuf>) -> serika_runtime::Result<PathBuf> {
    let path = project.unwrap_or(std::env::current_dir()?);
    if !path.is_dir() {
        return Err(Error::msg(format!("{} is not a directory", path.display())));
    }
    Ok(path)
}

async fn repl(
    runtime: &Runtime,
    project: &PathBuf,
    model: Option<String>,
    permission: Option<PermissionMode>,
    yes: bool,
    session: Option<String>,
    term: &Term,
) -> serika_runtime::Result<i32> {
    if !std::io::stdin().is_terminal() {
        return Err(Error::msg("no task was given and stdin is not a terminal"));
    }
    chat::run(runtime, project, model, permission, yes, session, *term).await
}
#[allow(clippy::too_many_arguments)]
async fn run_once(
    runtime: &Runtime,
    project: &PathBuf,
    prompt: &str,
    model: Option<String>,
    permission: Option<PermissionMode>,
    yes: bool,
    session: Option<String>,
    term: &Term,
) -> serika_runtime::Result<i32> {
    if prompt.trim().is_empty() {
        return Err(Error::msg("give serika a task, for example: serika \"Explain this project\""));
    }
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let cancel = CancellationToken::new();
    let cancel_for_task = cancel.clone();
    let approvals = runtime.approvals.clone();
    let task = runtime.run_task(TaskRequest {
        project: project.clone(),
        prompt: prompt.to_string(),
        session_id: session,
        model,
        permission,
        yes,
        events: tx,
        cancel: cancel_for_task,
        attachments: Vec::new(),
    });
    tokio::pin!(task);
    let mut failed: Option<String> = None;
    let mut renderer = TurnRenderer::new(*term);
    let mut ticker = tokio::time::interval(Duration::from_millis(80));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            biased;
            _ = tokio::signal::ctrl_c() => {
                cancel.cancel();
                approvals.deny_all();
                renderer.cancelled();
            }
            outcome = &mut task => {
                while let Ok(event) = rx.try_recv() {
                    render_event(event, &mut renderer, term, &approvals, &mut failed);
                }
                renderer.finish();
                return match outcome {
                    Ok(outcome) => {
                        if !outcome.finished {
                            eprintln!("{}", outcome.summary);
                            Ok(1)
                        } else {
                            Ok(0)
                        }
                    }
                    Err(err) => {
                        if failed.is_none() {
                            tui::error_line(term, &format!("serika: {err}"));
                        }
                        Ok(err.exit_code())
                    }
                };
            }
            Some(event) = rx.recv() => {
                render_event(event, &mut renderer, term, &approvals, &mut failed);
            }
            _ = ticker.tick() => {
                renderer.tick();
            }
        }
    }
}

fn render_event(
    event: AgentEvent,
    renderer: &mut TurnRenderer,
    term: &Term,
    approvals: &serika_runtime::ApprovalHub,
    failed: &mut Option<String>,
) {
    match event {
        AgentEvent::Started { session_id } => renderer.session_started(&session_id),
        AgentEvent::Status(text) => renderer.set_status(&text),
        AgentEvent::MessageChunk { text } => renderer.message_chunk(&text),
        AgentEvent::ReasoningChunk { .. } => {
            renderer.set_status("Thinking");
        }
        AgentEvent::MessageCompleted { .. } => renderer.message_completed(),
        AgentEvent::ToolRequested {
            id,
            name,
            arguments,
        } => renderer.tool_requested(&id, &name, &arguments),
        AgentEvent::ApprovalRequired {
            id,
            title,
            detail,
            destructive,
        } => {
            renderer.suspend();
            let allow = tui::ask_approval(term, &title, &detail, destructive);
            approvals.resolve(&id, allow);
            renderer.resume();
        }
        AgentEvent::ToolStarted { .. } => {}
        AgentEvent::ToolOutput { id, stream } => renderer.tool_output(&id, &stream),
        AgentEvent::ToolFinished {
            id,
            name,
            ok,
            summary,
        } => renderer.tool_finished(&id, &name, ok, &summary),
        AgentEvent::FileChanged(diff) => renderer.file_changed(&diff.path, &diff.diff),
        AgentEvent::UsageUpdated(usage) => renderer.usage(tui::usage_brief(&usage)),
        AgentEvent::Browser { frame } => renderer.set_status(&format!("Browsing {}", frame.url)),
        AgentEvent::Subagent { name, status, activity, .. } => {
            renderer.subagent(&name, &status, &activity)
        }
        AgentEvent::Handoff { from, to, messages } => {
            renderer.handoff(&tui::pretty_model(&from), &tui::pretty_model(&to), messages)
        }
        AgentEvent::ProviderUsage { usage, .. } => {
            if let Some(message) = usage.message {
                renderer.set_status(&message);
            }
        }
        AgentEvent::Completed { .. } => {}
        AgentEvent::Failed { message } => {
            renderer.failed(&message);
            *failed = Some(message);
        }
        AgentEvent::Cancelled => renderer.cancelled(),
    }
}

async fn auth(runtime: &Runtime, action: AuthCommand, term: &Term) -> serika_runtime::Result<i32> {
    match action {
        AuthCommand::Status => match runtime.signed_in()? {
            Some(session) => {
                println!(
                    "{} signed in as {} ({})",
                    term.green("●"),
                    term.bold(&session.username),
                    if session.is_premium { "premium" } else { "free" }
                );
                Ok(0)
            }
            None => {
                println!("not signed in (using local workspace / direct provider keys)");
                Ok(3)
            }
        },
        AuthCommand::Logout => {
            runtime.logout().await?;
            println!("signed out");
            Ok(0)
        }
        AuthCommand::Login => login(runtime).await,
    }
}

async fn login(runtime: &Runtime) -> serika_runtime::Result<i32> {
    if !std::io::stdin().is_terminal() {
        return Err(Error::msg("serika auth login needs a terminal"));
    }
    println!("Sign in to Serika Accounts");
    println!("1  password");
    println!("2  email link");
    eprint!("choice [1]: ");
    let _ = std::io::stdout().flush();
    let mut choice = String::new();
    std::io::stdin().read_line(&mut choice)?;
    if choice.trim() == "2" {
        return magic_login(runtime).await;
    }
    eprint!("email: ");
    let _ = std::io::stdout().flush();
    let mut email = String::new();
    std::io::stdin().read_line(&mut email)?;
    let mut password = rpassword::prompt_password("password: ").map_err(|err| Error::msg(err.to_string()))?;
    let outcome = runtime.login_password(email.trim(), &password, None).await?;
    password.zeroize();
    finish_factor(runtime, outcome, true, email.trim()).await
}

async fn magic_login(runtime: &Runtime) -> serika_runtime::Result<i32> {
    eprint!("email: ");
    let _ = std::io::stdout().flush();
    let mut email = String::new();
    std::io::stdin().read_line(&mut email)?;
    let message = runtime.request_magic_link(email.trim()).await?;
    println!("{message}");
    println!("Paste the token from the link (the part after /magic/).");
    eprint!("token: ");
    let _ = std::io::stdout().flush();
    let mut token = String::new();
    std::io::stdin().read_line(&mut token)?;
    let outcome = runtime.consume_magic_link(token.trim(), None).await?;
    finish_factor(runtime, outcome, false, token.trim()).await
}

async fn finish_factor(
    runtime: &Runtime,
    outcome: LoginOutcome,
    password_flow: bool,
    secret: &str,
) -> serika_runtime::Result<i32> {
    match outcome {
        LoginOutcome::SignedIn(session) => {
            println!("signed in as {}", session.username);
            Ok(0)
        }
        LoginOutcome::NeedsSecondFactor { message } => {
            println!("{message}");
            let code = rpassword::prompt_password("authentication code: ").map_err(|err| Error::msg(err.to_string()))?;
            let outcome = if password_flow {
                let mut password = rpassword::prompt_password("password again: ").map_err(|err| Error::msg(err.to_string()))?;
                let outcome = runtime.login_password(secret, &password, Some(code.trim())).await?;
                password.zeroize();
                outcome
            } else {
                runtime.consume_magic_link(secret, Some(code.trim())).await?
            };
            match outcome {
                LoginOutcome::SignedIn(session) => {
                    println!("signed in as {}", session.username);
                    Ok(0)
                }
                LoginOutcome::NeedsSecondFactor { message } => Err(Error::Auth(message)),
            }
        }
    }
}

async fn models(runtime: &Runtime, term: &Term) -> serika_runtime::Result<i32> {
    let report =
        serika_runtime::providers::discover_all_models(runtime.client.http(), &runtime.config)
            .await;
    println!(
        "\n  {}",
        term.bold("Local Workspace & Local Engines (No API Key Required)")
    );
    for model in report
        .models
        .iter()
        .filter(|m| m.provider.as_deref() == Some("local"))
    {
        print_model(term, model);
    }
    let api_models: Vec<_> = report
        .models
        .iter()
        .filter(|m| m.provider.as_deref() != Some("local"))
        .collect();
    if !api_models.is_empty() {
        println!(
            "\n  {}",
            term.bold("Live Provider API Endpoints (/v1/models · /v1beta/models)")
        );
        for model in api_models {
            print_model(term, model);
        }
    }
    if let Ok(catalog) = runtime.models().await {
        println!("\n  {}", term.bold("Serika Cloud (Free)"));
        for model in &catalog.free {
            print_model(term, model);
        }
        println!("\n  {}", term.bold("Serika Cloud (Premium)"));
        for model in &catalog.premium {
            print_model(term, model);
        }
    }
    println!();
    Ok(0)
}

fn print_model(term: &Term, model: &serika_runtime::ChatModel) {
    let mult = model
        .usage_multiplier
        .map(|value| format!("  ×{value}"))
        .unwrap_or_default();
    let mark = if model.is_default {
        format!("  {}", term.green("default"))
    } else {
        String::new()
    };
    println!(
        "  {:<38} {}{}{}",
        term.accent(&model.id),
        model.name,
        term.dim(&mult),
        mark
    );
}

async fn usage(runtime: &Runtime, term: &Term) -> serika_runtime::Result<i32> {
    let snapshot = runtime.usage().await?;
    println!("  {} {}", term.accent("•"), snapshot.summary_line());
    if let Some(plan) = &snapshot.plan {
        if let Some(offer) = &plan.reset_offer {
            if let Some(url) = &offer.url {
                print!("  weekly reset: {url}");
                if let Some(price) = &offer.price {
                    print!(" ({price})");
                }
                println!();
            }
        }
        if plan.byok == Some(true) {
            println!("  {}", term.dim("bring-your-own-key is enabled on this account; those requests are not metered"));
        }
    }
    Ok(0)
}

fn config_cmd(runtime: &Runtime, action: ConfigCommand) -> serika_runtime::Result<i32> {
    match action {
        ConfigCommand::Path => {
            println!("{}", runtime.paths.config_file.display());
            Ok(0)
        }
        ConfigCommand::Show => {
            println!("{}", serde_json::to_string_pretty(&runtime.config)?);
            Ok(0)
        }
        ConfigCommand::Set { key, value } => {
            let mut config = runtime.config.clone();
            config.set_field(&key, &value)?;
            config.save(&runtime.paths.config_file)?;
            println!("saved {key}");
            Ok(0)
        }
    }
}

async fn doctor(runtime: &Runtime, term: &Term) -> serika_runtime::Result<i32> {
    let report = runtime.doctor().await?;
    if !term.fancy {
        print!("{}", report.render());
    } else {
        println!("\n  {}", term.bold("Serika Code Environment Check"));
        for line in report.render().lines() {
            if line.starts_with("ok ") || line.contains("[ok]") {
                println!("  {} {}", term.green("✔"), line);
            } else if line.starts_with("warn") || line.contains("[warn]") {
                println!("  {} {}", term.amber("▲"), line);
            } else {
                println!("  {} {}", term.accent("•"), line);
            }
        }
        println!();
    }
    Ok(if report.ok() { 0 } else { 1 })
}

fn sessions(
    runtime: &Runtime,
    project: &PathBuf,
    action: SessionCommand,
    term: &Term,
) -> serika_runtime::Result<i32> {
    match action {
        SessionCommand::List => {
            let list = runtime.sessions.list(Some(project))?;
            if list.is_empty() {
                println!("no sessions in {}", project.display());
                return Ok(0);
            }
            for session in list {
                println!(
                    "  {}  {}  {}",
                    term.accent(&session.id),
                    term.dim(&session.permission),
                    session.title
                );
            }
            Ok(0)
        }
        SessionCommand::Rename { id, title } => {
            runtime.sessions.rename(&id, &title.join(" "))?;
            println!("renamed");
            Ok(0)
        }
        SessionCommand::Delete { id } => {
            runtime.sessions.delete(&id)?;
            println!("deleted");
            Ok(0)
        }
    }
}

async fn review(
    runtime: &Runtime,
    project: &PathBuf,
    ask: bool,
    focus: &[String],
    model: Option<String>,
    yes: bool,
    term: &Term,
) -> serika_runtime::Result<i32> {
    let status = serika_runtime::gitinfo::status(project).await?;
    let diff = serika_runtime::gitinfo::diff(project, None).await?;
    println!("{status}\n");
    if diff.trim().is_empty() || diff.contains("not a git") {
        println!("{diff}");
    } else {
        tui::print_diff(term, &diff);
    }
    if !ask {
        return Ok(0);
    }
    let focus = if focus.is_empty() {
        "Review the current git diff. Point out bugs and missing tests. Do not edit files.".to_string()
    } else {
        format!("Review the current git diff with this focus: {}. Do not edit files.", focus.join(" "))
    };
    run_once(
        runtime,
        project,
        &focus,
        model,
        Some(PermissionMode::ReadOnly),
        yes,
        None,
        term,
    )
    .await
}

async fn git_cmd(
    runtime: &Runtime,
    project: &PathBuf,
    action: GitCommand,
    term: &Term,
) -> serika_runtime::Result<i32> {
    use serika_runtime::gitinfo;
    match action {
        GitCommand::Status => {
            let snap = gitinfo::snapshot(project).await?;
            if !snap.is_repo {
                println!("{} is not a git repository", project.display());
                return Ok(1);
            }
            println!(
                "  {} {}  {}",
                term.dim("branch:"),
                term.accent(&snap.branch),
                term.dim(&format!("(ahead {}, behind {})", snap.ahead, snap.behind))
            );
            if snap.entries.is_empty() {
                println!("  {} working tree clean", term.green("✔"));
            } else {
                for entry in &snap.entries {
                    let state = if entry.staged && entry.has_unstaged_changes() {
                        "staged+modified"
                    } else if entry.staged {
                        "staged"
                    } else if entry.untracked {
                        "untracked"
                    } else {
                        "modified"
                    };
                    let badge = if entry.staged {
                        term.green(&format!("[{}]", entry.badge()))
                    } else {
                        term.amber(&format!("[{}]", entry.badge()))
                    };
                    println!("  {badge} {:<15} {}", term.dim(state), entry.path);
                }
            }
            Ok(0)
        }
        GitCommand::Branches => {
            let snap = gitinfo::snapshot(project).await?;
            for b in snap.branches {
                if b == snap.branch {
                    println!("* {}", term.accent(&b));
                } else {
                    println!("  {b}");
                }
            }
            Ok(0)
        }
        GitCommand::Checkout { branch, create } => {
            let out = gitinfo::checkout_branch(project, &branch, create).await?;
            println!("{out}");
            Ok(0)
        }
        GitCommand::Stage { path, all } => {
            let out = if all {
                gitinfo::stage_all(project).await?
            } else if let Some(path) = path {
                gitinfo::stage_file(project, &path).await?
            } else {
                return Err(Error::msg("pass a file path or --all"));
            };
            println!("{out}");
            Ok(0)
        }
        GitCommand::Unstage { path, all } => {
            let out = if all {
                gitinfo::unstage_all(project).await?
            } else if let Some(path) = path {
                gitinfo::unstage_file(project, &path).await?
            } else {
                return Err(Error::msg("pass a file path or --all"));
            };
            println!("{out}");
            Ok(0)
        }
        GitCommand::Commit { message, all } => {
            let out = if all {
                gitinfo::commit_all(project, &message).await?
            } else {
                gitinfo::commit_staged(project, &message).await?
            };
            println!("{out}");
            Ok(0)
        }
        GitCommand::Worktree { branch } => {
            let wt = gitinfo::create_worktree(project, &runtime.paths.worktrees_dir(), &branch).await?;
            println!("created worktree at {}", wt.display());
            Ok(0)
        }
        GitCommand::Push => {
            let out = gitinfo::push(project).await?;
            println!("{out}");
            Ok(0)
        }
        GitCommand::Pull => {
            let out = gitinfo::pull(project).await?;
            println!("{out}");
            Ok(0)
        }
    }
}

async fn update_cmd(check: bool, force: bool, term: &Term) -> serika_runtime::Result<i32> {
    let info = serika_runtime::updater::check_for_update().await?;
    println!("  {} {}", term.dim("current version :"), info.current_version);
    println!(
        "  {} {} ({})",
        term.dim("latest version  :"),
        term.accent(&info.latest_version),
        info.tag_name
    );
    if !info.notes.trim().is_empty() {
        println!(
            "  {} {}",
            term.dim("notes           :"),
            info.notes.lines().next().unwrap_or("")
        );
    }
    if check {
        if info.update_available {
            println!("update available: run `serika update` to install {}", info.tag_name);
        } else {
            println!("up to date");
        }
        return Ok(0);
    }
    if !info.update_available && !force {
        println!("Serika Code {} is already up to date.", info.current_version);
        return Ok(0);
    }
    println!("downloading {}...", info.asset_name);
    let outcome = serika_runtime::updater::install_update(&info, None, force).await?;
    println!(
        "updated {} -> {} in {} ({}){}",
        outcome.previous_version,
        outcome.installed_version,
        outcome.install_dir.display(),
        outcome.updated_binaries.join(", "),
        if outcome.sha256_verified {
            " [SHA-256 verified]"
        } else {
            ""
        }
    );
    Ok(0)
}

fn setup_cmd(
    runtime: &Runtime,
    prefix: Option<PathBuf>,
    no_path: bool,
    no_shortcuts: bool,
    uninstall: bool,
    purge: bool,
) -> serika_runtime::Result<i32> {
    if uninstall {
        let removed = serika_runtime::setup::uninstall_system(
            &runtime.paths,
            prefix.as_deref(),
            purge,
        )?;
        if removed.is_empty() {
            println!("nothing to remove");
        } else {
            for item in removed {
                println!("removed {item}");
            }
        }
        return Ok(0);
    }
    let report = serika_runtime::setup::install_system(
        &runtime.paths,
        &serika_runtime::setup::SetupOptions {
            prefix,
            add_to_path: !no_path,
            create_shortcuts: !no_shortcuts,
        },
    )?;
    print!("{}", report.render());
    Ok(0)
}

fn launch_desktop(project: &PathBuf, term: &Term) -> serika_runtime::Result<i32> {
    let bin_name = if cfg!(windows) { "serika-code.exe" } else { "serika-code" };
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(PathBuf::from));
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(dir) = &exe_dir {
        // Installed: <root>/bin/serika.exe with <root>/app/serika-code.exe.
        candidates.push(dir.join("..").join("app").join(bin_name));
        candidates.push(dir.join(bin_name));
        // Source checkout: target/<profile>/serika.exe with the packaged or built Qt app.
        candidates.push(dir.join("..").join("..").join("dist").join("serika-code").join(bin_name));
        candidates.push(
            dir.join("..")
                .join("..")
                .join("qt")
                .join("serika-code")
                .join("build")
                .join(bin_name),
        );
    }
    candidates.push(PathBuf::from("qt/serika-code/build").join(bin_name));
    let candidate = candidates
        .into_iter()
        .find(|path| path.is_file())
        .unwrap_or_else(|| PathBuf::from(bin_name));

    std::process::Command::new(candidate)
        .current_dir(project)
        .spawn()
        .map_err(|err| Error::msg(format!("could not start the Qt desktop: {err}")))?;
    println!(
        "{} started Serika Code desktop in {}",
        term.green("●"),
        project.display()
    );
    Ok(0)
}

fn exit_error(err: Error) -> ! {
    eprintln!("serika: {err}");
    std::process::exit(err.exit_code());
}
