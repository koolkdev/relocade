use std::{
    collections::HashSet,
    io::{BufRead, BufReader, BufWriter, Write},
    path::Path,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::{Mutex, OnceLock},
};

use crate::Module;
use serde::{de::DeserializeOwned, Deserialize, Serialize};

/// Execute an adapter's default export `(modules, input)` under TurboFan.
/// The first module selects a worker; all modules share that worker's compilation
/// cache. The adapter receives them in order and owns fresh instances per request.
pub fn run_v8<T: Serialize + ?Sized, R: DeserializeOwned>(
    script: &Path,
    modules: &[&Module],
    input: &T,
) -> R {
    let first = modules.first().expect("a V8 request needs a module");
    static WORKERS: OnceLock<Vec<Mutex<Option<Worker>>>> = OnceLock::new();
    let workers = WORKERS.get_or_init(|| {
        let count = std::thread::available_parallelism().map_or(1, |count| count.get().min(4));
        (0..count).map(|_| Mutex::new(None)).collect()
    });
    let mut slot = workers[first.id % workers.len()]
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    // A transport/protocol panic drops this worker, so later independent tests
    // start cleanly. An adapter error is a complete response and keeps it usable.
    let mut worker = slot.take().unwrap_or_else(Worker::start);
    let result = worker.run(script, modules, input);
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
        modules: &[&Module],
        input: &T,
    ) -> Result<R, String> {
        #[derive(Serialize)]
        struct Request<'a, T: ?Sized> {
            adapter: &'a Path,
            modules: Vec<ModuleSource<'a>>,
            input: &'a T,
        }

        #[derive(Serialize)]
        struct ModuleSource<'a> {
            id: usize,
            wasm: Option<&'a Path>,
        }

        // Transfer each uncached module once, keeping its file alive until the
        // worker responds. Repeated entries retain their positions in the list.
        let mut sent = HashSet::new();
        let files = modules
            .iter()
            .map(|module| {
                (!self.modules.contains(&module.id) && sent.insert(module.id)).then(|| {
                    let mut file = tempfile::Builder::new()
                        .prefix("wasm86-")
                        .suffix(".wasm")
                        .tempfile()
                        .expect("create V8 test module");
                    file.write_all(module.bytes())
                        .expect("write V8 test module");
                    file
                })
            })
            .collect::<Vec<_>>();
        let request = Request {
            adapter: script,
            modules: modules
                .iter()
                .zip(&files)
                .map(|(module, file)| ModuleSource {
                    id: module.id,
                    wasm: file.as_ref().map(|file| file.path()),
                })
                .collect(),
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
                self.modules.extend(modules.iter().map(|module| module.id));
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
