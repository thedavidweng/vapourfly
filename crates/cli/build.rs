use std::{fs, process::Command};

const RELEASE_GIT_HASH_FILE: &str = "release-git-hash";

fn main() {
    // Embed git commit hash (short)
    let git_hash = git_output(["rev-parse", "--short", "HEAD"])
        .or_else(release_git_hash)
        .unwrap_or_else(|| "unknown".to_string());

    // Embed build date (UTC, YYYY-MM-DD)
    let build_date = Command::new("date")
        .args(["-u", "+%Y-%m-%d"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map_or_else(
            || "unknown".to_string(),
            |o| String::from_utf8_lossy(&o.stdout).trim().to_string(),
        );

    println!("cargo:rustc-env=VF_GIT_HASH={git_hash}");
    println!("cargo:rustc-env=VF_BUILD_DATE={build_date}");
    println!("cargo:rerun-if-changed={RELEASE_GIT_HASH_FILE}");

    // Re-run when HEAD changes or the checked-out branch advances.
    if let Some(path) = git_output(["rev-parse", "--git-path", "HEAD"]) {
        println!("cargo:rerun-if-changed={path}");
    }
    if let Some(reference) = git_output(["symbolic-ref", "-q", "HEAD"]) {
        if let Some(path) = git_output(["rev-parse", "--git-path", &reference]) {
            println!("cargo:rerun-if-changed={path}");
        }
    }
    if let Some(path) = git_output(["rev-parse", "--git-path", "packed-refs"]) {
        println!("cargo:rerun-if-changed={path}");
    }
}

fn git_output<const N: usize>(args: [&str; N]) -> Option<String> {
    Command::new("git")
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

fn release_git_hash() -> Option<String> {
    fs::read_to_string(RELEASE_GIT_HASH_FILE)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}
