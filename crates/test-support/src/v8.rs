use std::{
    collections::HashSet,
    io::{BufRead, BufReader, BufWriter, Write},
    path::Path,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::{Mutex, OnceLock},
};

use crate::Module;
use serde::{de::DeserializeOwned, Deserialize, Serialize};

/// Execute an adapter's default export `(module, input)` under TurboFan.
/// A bounded pool caches compiled modules within the test process. Each module
/// stays on one worker; adapters create fresh execution state for each request.
pub(super) fn run<T: Serialize + ?Sized, R: DeserializeOwned>(
    script: &Path,
    module: &Module,
    input: &T,
) -> R {
    static WORKERS: OnceLock<Vec<Mutex<Option<Worker>>>> = OnceLock::new();
    let workers = WORKERS.get_or_init(|| {
        let count = std::thread::available_parallelism().map_or(1, |count| count.get().min(4));
        (0..count).map(|_| Mutex::new(None)).collect()
    });
    let mut slot = workers[module.id % workers.len()]
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    // A transport/protocol panic drops this worker, so later independent tests
    // start cleanly. An adapter error is a complete response and keeps it usable.
    let mut worker = slot.take().unwrap_or_else(Worker::start);
    let result = worker.run(script, module, input);
    *slot = Some(worker);
    drop(slot);
    result.unwrap_or_else(|error| panic!("{} failed: {error}", script.display()))
}

struct Worker {
    child: Child,
    input: BufWriter<ChildStdin>,
    output: BufReader<ChildStdout>,
    modules: HashSet<usize>,
}

#[derive(Deserialize)]
#[serde(tag = "status", content = "value", rename_all = "snake_case")]
enum Response<T> {
    Ok(T),
    Error(String),
}

impl Worker {
    fn start() -> Self {
        let mut child = Command::new("node")
            .args([
                "--no-liftoff",
                "--no-wasm-lazy-compilation",
                "--no-wasm-tier-up",
            ])
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/v8/worker.mjs"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("V8 tests require Node.js on PATH");
        Self {
            input: BufWriter::new(child.stdin.take().unwrap()),
            output: BufReader::new(child.stdout.take().unwrap()),
            child,
            modules: HashSet::new(),
        }
    }

    fn run<T: Serialize + ?Sized, R: DeserializeOwned>(
        &mut self,
        script: &Path,
        module: &Module,
        input: &T,
    ) -> Result<R, String> {
        #[derive(Serialize)]
        struct Request<'a, T: ?Sized> {
            adapter: &'a Path,
            module: usize,
            wasm: Option<&'a Path>,
            input: &'a T,
        }

        let cached = self.modules.contains(&module.id);
        // Transfer bytes only on the first use. Keep the file alive until the
        // worker has compiled it; subsequent requests carry just the module ID.
        let file = (!cached).then(|| {
            let mut file = tempfile::Builder::new()
                .prefix("wasm86-")
                .suffix(".wasm")
                .tempfile()
                .expect("create V8 test module");
            file.write_all(module.bytes())
                .expect("write V8 test module");
            file
        });
        let request = Request {
            adapter: script,
            module: module.id,
            wasm: file.as_ref().map(|file| file.path()),
            input,
        };
        serde_json::to_writer(&mut self.input, &request).expect("serialize V8 test input");
        self.input.write_all(b"\n").expect("send V8 test input");
        self.input.flush().expect("flush V8 test input");
        let mut line = String::new();
        let read = self
            .output
            .read_line(&mut line)
            .expect("read V8 observation");
        assert!(
            read != 0,
            "V8 worker exited without an observation; see stderr"
        );
        let response = serde_json::from_str(&line)
            .unwrap_or_else(|error| panic!("invalid V8 observation: {error}\n{line}"));
        match response {
            Response::Ok(value) => {
                self.modules.insert(module.id);
                Ok(value)
            }
            Response::Error(error) => Err(error),
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
