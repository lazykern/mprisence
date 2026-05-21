fn main() {
    // Try to get git SHA
    let sha = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "unknown".to_string());

    // Check if working tree is dirty
    let dirty = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .ok()
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false);

    let sha = if dirty {
        format!("{sha}-dirty")
    } else {
        sha
    };

    println!("cargo:rustc-env=GIT_SHA={sha}");
    println!("cargo:warning=git SHA: {sha}");

    // Re-run if git HEAD changes
    println!("cargo:rerun-if-changed=../../.git/HEAD");
}
