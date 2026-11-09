//! Versioned interpreter Wasm generated on demand by the execution owner.
//! Native builds persist artifacts in the user's cache directory; browsers use
//! CacheStorage. Unavailable caches regenerate without failing startup; native
//! reads also verify content integrity before reusing an artifact.
//! Each artifact exports both `step` and `run`; CPU features remain runtime inputs.

use std::future::Future;

pub use relocade_x86_profile::{ExecutionProfile, SegmentProfile};

mod cache;

/// Identifies the generator revision, ABI and execution assumptions of an artifact.
/// Use this key when retaining an engine-compiled module in memory too.
pub fn key(profile: ExecutionProfile) -> &'static str {
    match profile {
        ExecutionProfile::Protected(SegmentProfile::Flat32) => {
            concat!(
                "relocade/interpreter/",
                env!("RELOCADE_INTERPRETER_REVISION"),
                "/flat32"
            )
        }
        ExecutionProfile::Protected(SegmentProfile::Segmented32) => {
            concat!(
                "relocade/interpreter/",
                env!("RELOCADE_INTERPRETER_REVISION"),
                "/segmented32"
            )
        }
        ExecutionProfile::Protected(SegmentProfile::Segmented16) => {
            concat!(
                "relocade/interpreter/",
                env!("RELOCADE_INTERPRETER_REVISION"),
                "/segmented16"
            )
        }
        ExecutionProfile::Real16 => {
            concat!(
                "relocade/interpreter/",
                env!("RELOCADE_INTERPRETER_REVISION"),
                "/real16"
            )
        }
    }
}

/// Loads the requested interpreter, invoking `generate` only on a cache miss.
///
/// The callback must compile `relocade_x86::build_interpreter(profile)` with the
/// caller's executor or its serial compiler. Cached code retains the same profile
/// compatibility requirements as fresh code. Cache I/O is best effort; generation
/// errors are returned unchanged and failed generation is never cached.
/// The cache stores Wasm, leaving native machine-code caching to the Wasm engine.
pub async fn load_or_generate<E, F>(
    profile: ExecutionProfile,
    generate: impl FnOnce() -> F,
) -> Result<Vec<u8>, E>
where
    F: Future<Output = Result<Vec<u8>, E>>,
{
    cache::Cache::open()
        .await
        .load_or_generate(key(profile), generate)
        .await
}
