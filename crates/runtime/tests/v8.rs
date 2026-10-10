#[test]
#[ignore = "requires Node.js and the wasm32 codegen artifact; see runtime README"]
fn asynchronous_v8_runtime() {
    let status = std::process::Command::new("node")
        .args([
            "--no-liftoff",
            "--no-wasm-lazy-compilation",
            "--no-wasm-tier-up",
            "--test",
            "--test-isolation=none",
        ])
        .arg(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("v8/runtime.test.mjs"))
        .arg(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("v8/state.test.mjs"))
        .arg(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("v8/memory.test.mjs"))
        .arg(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("v8/physical.test.mjs"))
        .status()
        .expect("start Node.js 24");
    assert!(status.success(), "V8 runtime tests failed");
}
