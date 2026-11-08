//! Helpers shared by the integration tests.

use std::path::Path;
use std::process::Command;

/// Writes the shell script `body` to `path`, executable, for a test to run
/// (a fake kernel, say). A child `sh` writes it; this process never opens it
/// (#117).
///
/// Written here, the file's write descriptor would be open while the other
/// test threads fork, and a child forked then holds it until that child
/// execs (O_CLOEXEC closes it only at exec). Linux refuses to exec a file
/// that anything holds open for writing, so running the script could fail
/// with ETXTBSY ("Text file busy"). The descriptor lives only in the `sh`,
/// which has exited when this returns, and which no other child inherits.
pub fn executable(path: &Path, body: &str) {
    let status = Command::new("sh")
        .args([
            "-c",
            r#"printf '#!/bin/sh\n%s\n' "$2" > "$1" && chmod 755 "$1""#,
        ])
        .arg("sh")
        .arg(path)
        .arg(body)
        .status()
        .unwrap();
    assert!(status.success(), "could not write {}", path.display());
}
