use std::{
    fs,
    io::Write as _,
    path::PathBuf,
    process::{Command, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
};

pub(super) struct ModuleFile {
    path: PathBuf,
    pub(super) entry: String,
}

impl ModuleFile {
    pub(super) fn new(module: &crate::CompiledModule) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "wasm86-step-{}-{}.wasm",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::write(&path, &module.bytes).unwrap();
        Self {
            path,
            entry: module.entry.clone(),
        }
    }

    pub(super) fn observe(&self, flags: &[&str], input: &str, invocations: usize) -> String {
        let mut child = Command::new("node")
            .args(flags)
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/support/execute-step.mjs"
            ))
            .arg(&self.path)
            .arg(&self.entry)
            .arg(i64::MIN.to_string())
            .arg(invocations.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the explicit V8 lane requires Node.js on PATH");
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }
}

impl Drop for ModuleFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}
