#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod ui;

#[cfg(target_os = "windows")]
fn ensure_cli_output_visible() {
    const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
    const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
    extern "system" {
        fn GetStdHandle(nStdHandle: u32) -> isize;
        fn AttachConsole(dwProcessId: u32) -> i32;
    }
    unsafe {
        let handle = GetStdHandle(STD_OUTPUT_HANDLE);
        if handle == 0 || handle == -1 {
            let _ = AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn ensure_cli_output_visible() {}

fn main() {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("--version" | "-V") => {
            ensure_cli_output_visible();
            println!("serika-desktop {}", serika_runtime::client::VERSION);
        }
        Some("--help" | "-h") => {
            ensure_cli_output_visible();
            println!(
                "Serika Code desktop\n\n  \
                 serika-desktop              open the application\n  \
                 serika-desktop --smoke      open and close immediately\n  \
                 serika-desktop --setup      install binaries, PATH, and desktop shortcuts\n  \
                 serika-desktop --uninstall  remove installed binaries and shortcuts\n  \
                 serika-desktop --version"
            );
        }
        Some("--smoke") => ui::launch(true),
        Some("--setup") => {
            ensure_cli_output_visible();
            let paths = serika_runtime::AppPaths::resolve().unwrap_or_else(|err| {
                eprintln!("serika-desktop: {err}");
                std::process::exit(1);
            });
            match serika_runtime::setup::install_system(
                &paths,
                &serika_runtime::setup::SetupOptions::default(),
            ) {
                Ok(report) => print!("{}", report.render()),
                Err(err) => {
                    eprintln!("serika-desktop: {err}");
                    std::process::exit(1);
                }
            }
        }
        Some("--uninstall") => {
            ensure_cli_output_visible();
            let paths = serika_runtime::AppPaths::resolve().unwrap_or_else(|err| {
                eprintln!("serika-desktop: {err}");
                std::process::exit(1);
            });
            match serika_runtime::setup::uninstall_system(&paths, None, false) {
                Ok(removed) => {
                    for item in removed {
                        println!("removed {item}");
                    }
                }
                Err(err) => {
                    eprintln!("serika-desktop: {err}");
                    std::process::exit(1);
                }
            }
        }
        Some(other) => {
            ensure_cli_output_visible();
            eprintln!("serika-desktop: unknown argument {other}");
            std::process::exit(2);
        }
        None => ui::launch(false),
    }
}
