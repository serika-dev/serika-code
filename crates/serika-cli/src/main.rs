use std::io::{IsTerminal, Write};
use std::path::PathBuf;

use clap::{Parser, Subcommand};
use serika_runtime::{
    AgentEvent, Error, LoginOutcome, PermissionMode, Runtime, TaskRequest,
};
use tokio_util::sync::CancellationToken;
use zeroize::Zeroize;

#[derive(Parser)]
#[command(
    name = "serika",
    version,
    about = "Serika Code — a native coding agent for SerikaAI"
)]
struct Cli {
    /// Model id from `serika models`. Defaults to a coding model in the live catalog.
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
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Interactive session in the current project.
    Chat,
    /// Run one task and exit.
    Run { prompt: Vec<String> },
    /// Sign in, sign out, or show who is signed in.
    Auth {
        #[command(subcommand)]
        action: AuthCommand,
    },
    /// List models the account can see.
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
    /// A task written directly after `serika`.
    #[command(external_subcommand)]
    Task(Vec<String>),
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
    let code = match dispatch(runtime, cli).await {
        Ok(code) => code,
        Err(err) => {
            eprintln!("serika: {err}");
            err.exit_code()
        }
    };
    std::process::exit(code);
}

async fn dispatch(runtime: Runtime, cli: Cli) -> serika_runtime::Result<i32> {
    let project = project_path(cli.project)?;
    let permission = match cli.permission {
        Some(value) => Some(
            PermissionMode::parse(&value)
                .ok_or_else(|| Error::msg("permission must be read_only, standard, restricted, or full"))?,
        ),
        None => None,
    };
    match cli.command {
        None => repl(&runtime, &project, cli.model, permission, cli.yes, cli.session).await,
        Some(Commands::Chat) => repl(&runtime, &project, cli.model, permission, cli.yes, cli.session).await,
        Some(Commands::Run { prompt }) => {
            run_once(&runtime, &project, &prompt.join(" "), cli.model, permission, cli.yes, cli.session).await
        }
        Some(Commands::Task(prompt)) => {
            run_once(&runtime, &project, &prompt.join(" "), cli.model, permission, cli.yes, cli.session).await
        }
        Some(Commands::Auth { action }) => auth(&runtime, action).await,
        Some(Commands::Models) => models(&runtime).await,
        Some(Commands::Usage) => usage(&runtime).await,
        Some(Commands::Config { action }) => config_cmd(&runtime, action),
        Some(Commands::Doctor) => doctor(&runtime).await,
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
            repl(&runtime, &project, cli.model, permission, cli.yes, Some(id)).await
        }
        Some(Commands::Sessions { action }) => sessions(&runtime, &project, action),
        Some(Commands::Review { ask, focus }) => review(&runtime, &project, ask, &focus, cli.model, cli.yes).await,
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
    mut session: Option<String>,
) -> serika_runtime::Result<i32> {
    if !std::io::stdin().is_terminal() {
        return Err(Error::msg("no task was given and stdin is not a terminal"));
    }
    let who = runtime
        .signed_in()?
        .map(|session| session.username.clone())
        .unwrap_or_else(|| "not signed in".into());
    println!("Serika Code {}", serika_runtime::client::VERSION);
    println!("Project: {}", project.display());
    println!("Account: {who}");
    println!("Type a task, /exit to leave, /help for commands.");
    loop {
        print!("\n> ");
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        if std::io::stdin().read_line(&mut line).unwrap_or(0) == 0 {
            break;
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line == "/exit" || line == "/quit" {
            break;
        }
        if line == "/help" {
            println!("/exit  /usage  /models  /sessions");
            println!("Ctrl+C cancels the current task.");
            continue;
        }
        if line == "/usage" {
            usage(runtime).await?;
            continue;
        }
        if line == "/models" {
            models(runtime).await?;
            continue;
        }
        if line == "/sessions" {
            sessions(runtime, project, SessionCommand::List)?;
            continue;
        }
        match run_once(runtime, project, line, model.clone(), permission, yes, session.clone()).await {
            Ok(0) => {
                if session.is_none() {
                    session = runtime.sessions.list(Some(project))?.into_iter().next().map(|item| item.id);
                }
            }
            Ok(code) => return Ok(code),
            Err(Error::Cancelled) => println!("cancelled"),
            Err(err) => {
                eprintln!("serika: {err}");
                if matches!(err, Error::AuthRequired | Error::Auth(_)) {
                    return Err(err);
                }
            }
        }
    }
    Ok(0)
}

async fn run_once(
    runtime: &Runtime,
    project: &PathBuf,
    prompt: &str,
    model: Option<String>,
    permission: Option<PermissionMode>,
    yes: bool,
    session: Option<String>,
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
    });
    tokio::pin!(task);
    let mut failed: Option<String> = None;
    loop {
        tokio::select! {
            biased;
            _ = tokio::signal::ctrl_c() => {
                cancel.cancel();
                approvals.deny_all();
                println!();
            }
            outcome = &mut task => {
                while let Ok(event) = rx.try_recv() {
                    render(event, &approvals, &mut failed);
                }
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
                            eprintln!("serika: {err}");
                        }
                        Ok(err.exit_code())
                    }
                };
            }
            event = rx.recv() => {
                match event {
                    Some(event) => render(event, &approvals, &mut failed),
                    None => {}
                }
            }
        }
    }
}

fn render(event: AgentEvent, approvals: &serika_runtime::ApprovalHub, failed: &mut Option<String>) {
    match event {
        AgentEvent::Started { session_id } => eprintln!("{}", dim(&format!("session {session_id}"))),
        AgentEvent::Status(text) => eprintln!("{}", dim(&text)),
        AgentEvent::MessageChunk { text } => {
            print!("{text}");
            let _ = std::io::stdout().flush();
        }
        AgentEvent::ReasoningChunk { .. } => {}
        AgentEvent::MessageCompleted { .. } => println!(),
        AgentEvent::ToolRequested { name, .. } => eprintln!("{}", dim(&format!("tool {name}"))),
        AgentEvent::ApprovalRequired { id, title, detail, destructive } => {
            let allow = ask_approval(&title, &detail, destructive);
            approvals.resolve(&id, allow);
        }
        AgentEvent::ToolStarted { .. } => {}
        AgentEvent::ToolOutput { stream, .. } => eprintln!("{stream}"),
        AgentEvent::ToolFinished { name, ok, summary, .. } => {
            let mark = if ok { "ok" } else { "failed" };
            eprintln!("{}", dim(&format!("{name} {mark}: {summary}")));
        }
        AgentEvent::FileChanged(diff) => {
            eprintln!("{}", dim(&format!("changed {}", diff.path)));
            print_diff(&diff.diff);
        }
        AgentEvent::UsageUpdated(usage) => eprintln!("{}", dim(&usage.summary_line())),
        AgentEvent::Completed { .. } => {}
        AgentEvent::Failed { message } => {
            eprintln!("serika: {message}");
            *failed = Some(message);
        }
        AgentEvent::Cancelled => eprintln!("cancelled"),
    }
}

fn ask_approval(title: &str, detail: &str, destructive: bool) -> bool {
    if !std::io::stdin().is_terminal() {
        eprintln!("approval required to {title}: {detail}");
        return false;
    }
    let kind = if destructive { "This can be destructive. " } else { "" };
    eprint!("{kind}Allow {title}?\n{detail}\n[y/N] ");
    let _ = std::io::stderr().flush();
    let mut line = String::new();
    let _ = std::io::stdin().read_line(&mut line);
    matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

fn print_diff(diff: &str) {
    if !std::io::stdout().is_terminal() {
        println!("{diff}");
        return;
    }
    for line in diff.lines() {
        if line.starts_with('+') && !line.starts_with("+++") {
            println!("\x1b[32m{line}\x1b[0m");
        } else if line.starts_with('-') && !line.starts_with("---") {
            println!("\x1b[31m{line}\x1b[0m");
        } else {
            println!("{line}");
        }
    }
}

fn dim(text: &str) -> String {
    if std::io::stderr().is_terminal() {
        format!("\x1b[2m{text}\x1b[0m")
    } else {
        text.to_string()
    }
}

async fn auth(runtime: &Runtime, action: AuthCommand) -> serika_runtime::Result<i32> {
    match action {
        AuthCommand::Status => match runtime.signed_in()? {
            Some(session) => {
                println!(
                    "signed in as {} ({})",
                    session.username,
                    if session.is_premium { "premium" } else { "free" }
                );
                Ok(0)
            }
            None => {
                println!("not signed in");
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

async fn models(runtime: &Runtime) -> serika_runtime::Result<i32> {
    let catalog = runtime.models().await?;
    println!("free");
    for model in &catalog.free {
        print_model(model);
    }
    println!("\npremium");
    for model in &catalog.premium {
        print_model(model);
    }
    if let Some(default) = catalog.default_coding_model() {
        println!("\ndefault for new sessions: {default}");
    }
    Ok(0)
}

fn print_model(model: &serika_runtime::ChatModel) {
    let mult = model
        .usage_multiplier
        .map(|value| format!("  ×{value}"))
        .unwrap_or_default();
    let mark = if model.is_default { "  default" } else { "" };
    println!("  {}  {}{mult}{mark}", model.id, model.name);
}

async fn usage(runtime: &Runtime) -> serika_runtime::Result<i32> {
    let snapshot = runtime.usage().await?;
    println!("{}", snapshot.summary_line());
    if let Some(plan) = &snapshot.plan {
        if let Some(offer) = &plan.reset_offer {
            if let Some(url) = &offer.url {
                print!("weekly reset: {url}");
                if let Some(price) = &offer.price {
                    print!(" ({price})");
                }
                println!();
            }
        }
        if plan.byok == Some(true) {
            println!("bring-your-own-key is enabled on this account; those requests are not metered");
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

async fn doctor(runtime: &Runtime) -> serika_runtime::Result<i32> {
    let report = runtime.doctor().await?;
    print!("{}", report.render());
    Ok(if report.ok() { 0 } else { 1 })
}

fn sessions(runtime: &Runtime, project: &PathBuf, action: SessionCommand) -> serika_runtime::Result<i32> {
    match action {
        SessionCommand::List => {
            let list = runtime.sessions.list(Some(project))?;
            if list.is_empty() {
                println!("no sessions in {}", project.display());
                return Ok(0);
            }
            for session in list {
                println!("{}  {}  {}", session.id, session.permission, session.title);
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
) -> serika_runtime::Result<i32> {
    let status = serika_runtime::gitinfo::status(project).await?;
    let diff = serika_runtime::gitinfo::diff(project, None).await?;
    println!("{status}\n");
    if diff.trim().is_empty() || diff.contains("not a git") {
        println!("{diff}");
    } else {
        print_diff(&diff);
    }
    if !ask {
        return Ok(0);
    }
    let focus = if focus.is_empty() {
        "Review the current git diff. Point out bugs and missing tests. Do not edit files.".to_string()
    } else {
        format!("Review the current git diff with this focus: {}. Do not edit files.", focus.join(" "))
    };
    run_once(runtime, project, &focus, model, Some(PermissionMode::ReadOnly), yes, None).await
}

fn exit_error(err: Error) -> ! {
    eprintln!("serika: {err}");
    std::process::exit(err.exit_code());
}
