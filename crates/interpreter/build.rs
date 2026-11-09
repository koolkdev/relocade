use futures_util::future::join;
use relocade_workers::{Pool, Thread};
use relocade_x86::{build_interpreter, ExecutionProfile, SegmentProfile};
use std::{env, error::Error, fs, path::PathBuf, thread};

fn main() -> Result<(), Box<dyn Error>> {
    // Read Cargo's inherited descriptors before opening files or starting threads.
    // SAFETY: Cargo owns the advertised jobserver descriptors at process startup.
    let jobserver = unsafe { jobserver::Client::from_env() };
    println!("cargo:rerun-if-changed=build.rs");
    let parallelism = thread::available_parallelism().map_or(1, usize::from);
    let mut tokens = Vec::new();
    if let Some(jobserver) = jobserver {
        for _ in 1..parallelism {
            // Never wait for another slot: a build with -j1 must make progress.
            let Ok(Some(token)) = jobserver.try_acquire() else {
                break;
            };
            tokens.push(token);
        }
    }
    // The build script has one implicit slot. Retain extra leases until the pool
    // drains; the coordinator only constructs bodies between worker phases.
    let threads = (0..=tokens.len()).map(|_| Thread::new()).collect();
    let (pool, executor) = Pool::new(threads)?;
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo output directory"));
    let ((), result) = pollster::block_on(join(pool.run(), async move {
        for (name, profile) in [
            ("flat32", SegmentProfile::Flat32.into()),
            ("segmented32", SegmentProfile::Segmented32.into()),
            ("segmented16", SegmentProfile::Segmented16.into()),
            ("real16", ExecutionProfile::Real16),
        ] {
            let program = build_interpreter(profile)?;
            let bytes = program.compile_with(&executor).await?;
            fs::write(out.join(format!("{name}.wasm")), bytes)?;
        }
        Ok::<(), Box<dyn Error>>(())
    }));
    result
}
