use std::process::Command;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let local_name = if cfg!(windows) {
        "localagent.exe"
    } else {
        "localagent"
    };

    let status = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(local_name)))
        .filter(|p| p.exists())
        .map(|p| Command::new(p).args(&args).status())
        .unwrap_or_else(|| Command::new("localagent").args(&args).status());

    match status {
        Ok(s) => std::process::exit(s.code().unwrap_or(1)),
        Err(e) => {
            eprintln!("failed to launch localagent: {e}");
            std::process::exit(1);
        }
    }
}
