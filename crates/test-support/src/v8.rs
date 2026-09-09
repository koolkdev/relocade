use std::{
    io::Write,
    path::Path,
    process::{Command, Stdio},
};

use serde::{de::DeserializeOwned, Serialize};

/// Execute a host adapter under TurboFan. The adapter reads its typed request
/// from stdin and writes one JSON observation to stdout.
pub fn run_v8<T: Serialize + ?Sized, R: DeserializeOwned>(
    script: &Path,
    module: &[u8],
    input: &T,
) -> R {
    let mut file = tempfile::Builder::new()
        .prefix("wasm86-")
        .suffix(".wasm")
        .tempfile()
        .expect("create V8 test module");
    file.write_all(module).expect("write V8 test module");
    file.flush().expect("flush V8 test module");
    let request = serde_json::to_vec(input).expect("serialize V8 test input");
    let mut child = Command::new("node")
        .args([
            "--no-liftoff",
            "--no-wasm-lazy-compilation",
            "--no-wasm-tier-up",
        ])
        .arg(script)
        .arg(file.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("V8 tests require Node.js on PATH");
    let written = child.stdin.take().unwrap().write_all(&request);
    let output = child.wait_with_output().expect("wait for V8 test adapter");
    assert!(
        output.status.success(),
        "{} failed: {}",
        script.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    written.expect("send V8 test input");
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{} returned invalid observations: {error}\n{}",
            script.display(),
            String::from_utf8_lossy(&output.stdout)
        )
    })
}
