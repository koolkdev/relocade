//! A minimal DOS environment for one real-mode COM program.
//!
//! Programs start at PSP:0100 with a 64-KiB allocation and an empty command tail.
//! INT 21h supports AH=09h byte output and AH=4Ch termination. INT 20h also exits,
//! including a COM program's initial near RET. Output is buffered as guest bytes.
//! [`Dos::new`] prepares the DOS environment; [`Dos::load_com`] loads a program
//! into it. The host chooses the Wasm backend and controls execution through
//! [`Dos::runtime`]. Create the backend with its `with_state` constructor and
//! [`DosState::default()`].
//!
//! ```no_run
//! # async fn example<B>(backend: B) -> Result<(), Box<dyn std::error::Error>>
//! # where B: wasm86_runtime::RuntimeBackend<State = wasm86_dos::DosState>
//! # {
//! let mut dos = wasm86_dos::Dos::new(backend).await?;
//! dos.load_com(&[0xb8, 7, 0x4c, 0xcd, 0x21])?;
//! assert_eq!(dos.runtime().run(u64::MAX)?, wasm86_x86::ExecutionExit::HostReturn);
//! assert_eq!(dos.exit_status(), Some(Ok(7)));
//! let output: Vec<u8> = dos.take_output();
//! # let _ = output;
//! # Ok(())
//! # }
//! ```
#![forbid(unsafe_code)]

mod com;
mod error;
mod kernel;

pub use error::{LoadError, ServiceError};
pub use kernel::DosState;
use wasm86_runtime::{Runtime, RuntimeBackend};

/// A DOS environment and its runtime, with space for one loaded program.
pub struct Dos<B: RuntimeBackend<State = DosState>> {
    runtime: Runtime<B>,
}

impl<B: RuntimeBackend<State = DosState>> Dos<B> {
    /// Prepares the memory map, interrupt vectors and DOS service hooks.
    /// Load a program before entering guest execution.
    pub async fn new(backend: B) -> wasm86_runtime::Result<Self> {
        let mut runtime = Runtime::new(backend, kernel::configuration()).await?;
        kernel::initialize(&mut runtime.machine().memory());
        runtime.install_hooks(kernel::hooks()).await?;
        Ok(Self { runtime })
    }

    /// Loads a COM image of 1..=65,278 bytes and initializes its PSP, stack and CPU.
    /// Replaces the process allocation and clears prior output and exit status.
    /// Invalid images leave the existing program and its state unchanged.
    pub fn load_com(&mut self, image: &[u8]) -> Result<(), LoadError> {
        com::load(&mut self.runtime.machine(), image)?;
        *self.runtime.state_mut() = DosState::default();
        Ok(())
    }

    pub fn runtime(&mut self) -> &mut Runtime<B> {
        &mut self.runtime
    }

    /// None until a DOS service terminates the process or reports a service error.
    pub fn exit_status(&self) -> Option<Result<u8, ServiceError>> {
        self.runtime.state().exit_status
    }

    /// Drains output without interpreting the program's character encoding.
    pub fn take_output(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.runtime.state_mut().output)
    }
}
