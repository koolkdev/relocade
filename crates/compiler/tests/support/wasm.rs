use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

pub struct ModuleFile(PathBuf);

impl ModuleFile {
    pub fn new(bytes: &[u8]) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "wasm86-test-{}-{}.wasm",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::write(&path, bytes).unwrap();
        Self(path)
    }

    fn run(&self, flags: &[&str], adapter: &str, args: &[&str]) -> String {
        let output = Command::new("node")
            .args(flags)
            .arg(
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/support")
                    .join(adapter),
            )
            .arg(&self.0)
            .args(args)
            .output()
            .expect("the explicit V8 lane requires Node.js on PATH");
        assert!(
            output.status.success(),
            "{adapter}, args {args:?}, V8 flags {flags:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }

    #[track_caller]
    pub fn check(&self, flags: &[&str], adapter: &str, args: &[&str], expected: &str) {
        assert_eq!(
            self.run(flags, adapter, args),
            expected,
            "{adapter}, args {args:?}, V8 flags {flags:?}"
        );
    }
}

impl Drop for ModuleFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
