use std::process::Command;

fn main() {
    let git_sha = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                String::from_utf8(o.stdout).ok()
            } else {
                None
            }
        })
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string());

    let target = std::env::var("TARGET").unwrap_or_else(|_| "unknown".to_string());
    let build_time =
        std::env::var("OPENAGENT_BUILD_TIME_UTC").unwrap_or_else(|_| "unknown".to_string());

    println!("cargo:rustc-env=OPENAGENT_GIT_SHA={git_sha}");
    println!("cargo:rustc-env=OPENAGENT_TARGET={target}");
    println!("cargo:rustc-env=OPENAGENT_BUILD_TIME_UTC={build_time}");
}
