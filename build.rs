use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs");

    let version = Command::new("git")
        .args(["describe", "--tags"])
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .unwrap_or(String::from("unknown"));

    println!("cargo:rustc-env=CHATTY_VERSION={version}");
}
