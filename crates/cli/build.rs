use std::process::Command;

fn main() {
    // Embed git commit hash (short)
    let git_hash = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    // Embed build date (UTC, YYYY-MM-DD)
    let build_date = Command::new("date")
        .args(["-u", "+%Y-%m-%d"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    println!("cargo:rustc-env=VF_GIT_HASH={git_hash}");
    println!("cargo:rustc-env=VF_BUILD_DATE={build_date}");

    // Re-run if HEAD changes
    println!("cargo:rerun-if-changed=../../.git/HEAD");
}
