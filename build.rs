//! Puts the icon and the version information into the Windows executables, where Explorer,
//! the taskbar and "Open with" read them, and the commit they are built from into `RAWV_COMMIT`.

use std::process::Command;

fn main() -> std::io::Result<()> {
    println!("cargo:rerun-if-changed=assets/rawv.ico");
    for watched in ["src", ".git/HEAD", ".git/index"] {
        println!("cargo:rerun-if-changed={watched}");
    }
    println!("cargo:rustc-env=RAWV_COMMIT={}", commit());
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return Ok(());
    }
    let mut resource = winresource::WindowsResource::new();
    resource
        .set_icon("assets/rawv.ico")
        .set("FileDescription", "rawv")
        .set("ProductName", "rawv");
    resource.compile()
}

/// The short hash of the checked-out commit, with `+` when tracked files differ from it;
/// "unknown" outside a git checkout.
fn commit() -> String {
    let git = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| String::from_utf8(output.stdout).ok())
    };
    let Some(hash) = git(&["rev-parse", "--short=7", "HEAD"]) else {
        return "unknown".to_owned();
    };
    let changed = git(&["status", "--porcelain", "--untracked-files=no"])
        .is_some_and(|status| !status.trim().is_empty());
    format!("{}{}", hash.trim(), if changed { "+" } else { "" })
}
