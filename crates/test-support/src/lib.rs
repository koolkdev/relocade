//! Execution tools shared by the compiler and x86 test hosts.

mod v8;
mod value;

use std::sync::OnceLock;
use wasmtime::{Config, Engine};

pub use v8::run_v8;
pub use value::{decimal_i64, Outcome, Value};

/// Share engine configuration and compilation resources within a test process.
/// Hosts create a fresh store and instance for each independent case.
pub fn engine() -> &'static Engine {
    static ENGINE: OnceLock<Engine> = OnceLock::new();
    ENGINE.get_or_init(|| {
        let mut config = Config::new();
        config.wasm_multi_memory(true).wasm_tail_call(true);
        Engine::new(&config).expect("the test Wasm features must be supported")
    })
}
