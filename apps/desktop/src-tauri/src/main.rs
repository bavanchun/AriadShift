#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
fn main() -> std::process::ExitCode {
    if std::env::args_os().nth(1).is_some_and(|s| s == "__sandbox") {
        return ariad_host::sandbox::launch(std::env::args_os().skip(2));
    }
    let mut args = std::env::args_os().skip(1);
    if args.next().is_some_and(|arg| arg == "__engine") {
        let engine = args.next();
        let Some(engine) = engine.as_ref().and_then(|engine| engine.to_str()) else {
            eprintln!("Expected a built-in engine name");
            return std::process::ExitCode::from(2);
        };
        if args.next().is_some() {
            eprintln!("Unexpected built-in engine argument");
            return std::process::ExitCode::from(2);
        }
        return ariad_host::engines::serve_builtin(
            engine,
            std::io::stdin().lock(),
            std::io::stdout().lock(),
        );
    }
    ariad_desktop::run();
    std::process::ExitCode::SUCCESS
}
