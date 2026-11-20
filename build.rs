use std::env;
use std::path::PathBuf;
use std::process::Command;

const CARGO_PKG_VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=PROFILE");

    let release_build = env::var("PROFILE").as_deref() == Ok("release");
    let git_hash = if release_build {
        None
    } else {
        git(&["rev-parse", "--short=8", "HEAD"])
    };

    let display_version = match git_hash {
        Some(hash) => format!("{CARGO_PKG_VERSION} ({hash})"),
        None => CARGO_PKG_VERSION.to_string(),
    };
    println!("cargo:rustc-env=VERSION_AND_GIT_HASH={display_version}");

    let Some(git_dir) = git(&["rev-parse", "--git-dir"]).map(PathBuf::from) else {
        return;
    };

    let head = git_dir.join("HEAD");
    if head.exists() {
        println!("cargo:rerun-if-changed={}", head.display());
    }

    if let Some(head_ref) = git(&["symbolic-ref", "-q", "HEAD"]) {
        let head_ref = git_dir.join(head_ref);
        if head_ref.exists() {
            println!("cargo:rerun-if-changed={}", head_ref.display());
        }
    }
}

fn git(arguments: &[&str]) -> Option<String> {
    Command::new("git")
        .args(arguments)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|output| output.trim().to_string())
        .filter(|output| !output.is_empty())
}
