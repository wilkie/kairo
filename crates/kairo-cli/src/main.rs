use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);

    match args.next().as_deref() {
        Some("--version") | Some("version") => {
            println!("kairo {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("--help") | Some("help") | None => {
            print_help();
            ExitCode::SUCCESS
        }
        Some(command) => {
            eprintln!("unsupported command: {command}");
            ExitCode::from(2)
        }
    }
}

fn print_help() {
    println!("kairo");
    println!();
    println!("Usage:");
    println!("  kairo --help");
    println!("  kairo --version");
}
