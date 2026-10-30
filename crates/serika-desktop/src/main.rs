mod ui;

fn main() {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("--version" | "-V") => {
            println!("serika-desktop {}", serika_runtime::client::VERSION);
        }
        Some("--help" | "-h") => {
            println!("Serika Code desktop\n\n  serika-desktop           open the application\n  serika-desktop --smoke   open and close immediately\n  serika-desktop --version");
        }
        Some("--smoke") => ui::launch(true),
        Some(other) => {
            eprintln!("serika-desktop: unknown argument {other}");
            std::process::exit(2);
        }
        None => ui::launch(false),
    }
}
