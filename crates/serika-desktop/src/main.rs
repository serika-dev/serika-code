mod ui;

fn main() {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("--version" | "-V") => {
            println!("serika-desktop {}", serika_runtime::client::VERSION);
        }
        Some("--help" | "-h") => {
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
            eprintln!("serika-desktop: unknown argument {other}");
            std::process::exit(2);
        }
        None => ui::launch(false),
    }
}
