#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
fn main() -> std::process::ExitCode {
    if std::env::args_os().nth(1).is_some_and(|s| s == "__sandbox") {
        return ariad_host::sandbox::launch(std::env::args_os().skip(2));
    }
    if std::env::args().skip(1).collect::<Vec<_>>() == ["__engine", "pandoc"] {
        return ariad_host::engines::pandoc::serve(
            std::io::stdin().lock(),
            std::io::stdout().lock(),
        );
    }
    ariad_desktop::run();
    std::process::ExitCode::SUCCESS
}
