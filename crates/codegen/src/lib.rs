//! Owned compilation requests for background workers on native and Wasm hosts.
//! Generation never accesses live CPU or guest memory. Engine compilation and
//! scheduling belong to the embedding runtime.

#[cfg(target_arch = "wasm32")]
mod transport;

use serde::{Deserialize, Serialize};
use wasm86_x86::{CompiledModule, Compiler, ExecutionProfile, SegmentProfile};

/// Execution assumptions included in every request and cache owner.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Profile {
    Flat32,
    Segmented32,
    Segmented16,
    Real16,
}

impl From<Profile> for ExecutionProfile {
    fn from(profile: Profile) -> Self {
        match profile {
            Profile::Flat32 => SegmentProfile::Flat32.into(),
            Profile::Segmented32 => SegmentProfile::Segmented32.into(),
            Profile::Segmented16 => SegmentProfile::Segmented16.into(),
            Profile::Real16 => Self::Real16,
        }
    }
}

/// Explicit compilation work. Automatic hotness selection is left to a future
/// execution policy. Code is an owned snapshot, never shared guest RAM.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Entry {
    Interpreter,
    Block {
        eip: u32,
        code: Vec<u8>,
        instruction_limit: u32,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Request {
    pub profile: Profile,
    #[serde(flatten)]
    pub entry: Entry,
}

/// Maximum input and generation bounds for a single queued block.
pub const MAX_CODE_BYTES: usize = 4096;
pub const MAX_INSTRUCTIONS: u32 = 256;

impl Request {
    /// Generates a budgeted entry. Call only on the compilation worker.
    /// The host owns captured-code lifetime and rejects invalidated tickets.
    pub fn generate(&self) -> Result<CompiledModule, String> {
        let compiler = Compiler::new(ExecutionProfile::from(self.profile)).with_code_tracking();
        match &self.entry {
            Entry::Interpreter => compiler
                .compile_interpreter()
                .map_err(|error| error.to_string()),
            Entry::Block {
                eip,
                code,
                instruction_limit,
            } => {
                if code.len() > MAX_CODE_BYTES
                    || !(1..=MAX_INSTRUCTIONS).contains(instruction_limit)
                {
                    return Err("block exceeds the compilation limits".into());
                }
                compiler
                    .compile_block(*eip, code, *instruction_limit)
                    .map_err(|error| error.to_string())
            }
        }
    }
}
