//! A minimal DOS environment for one real-mode COM or MZ program.
//!
//! COM programs start at PSP:0100 with an empty command tail. They receive the
//! available conventional memory; the image and initial stack use its first 64 KiB.
//! MZ executables use their header's memory request, relocation table and CS:IP/SS:SP.
//! INT 21h supports AH=09h string output, AH=40h stdout/stderr writes and
//! AH=4Ch termination. INT 20h also exits,
//! including a COM program's initial near RET. Output is buffered as guest bytes.
//! AH=48h/49h/4Ah allocate, free and resize conventional-memory blocks.
//! [`Dos::mount_file`] supplies files for AH=3Dh/3Eh/3Fh/40h open/close/read/write.
//! AH=3Ch creates or truncates files in existing directories.
//! AH=42h seeks within those files relative to their start, current position or end.
//! [`Dos::new`] prepares the DOS environment. Load a program with [`Dos::load_com`]
//! or [`Dos::load_mz`]. The host chooses the Wasm backend and controls execution through
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
mod files;
mod filesystem;
mod interrupt;
mod kernel;
mod memory;
mod mz;
mod process;
mod services;

pub use error::{LoadError, MountError, ServiceError};
pub use filesystem::FileAccess;
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
        self.runtime.state_mut().reset_process();
        Ok(())
    }

    /// Loads a DOS MZ executable, applies relocations and initializes its PSP and CPU.
    /// Replaces the root process and clears prior output and exit status.
    /// Invalid images leave the existing program and its state unchanged.
    /// Overlay loading is not supported.
    pub fn load_mz(&mut self, image: &[u8]) -> Result<(), LoadError> {
        mz::load(&mut self.runtime.machine(), image)?;
        self.runtime.state_mut().reset_process();
        Ok(())
    }

    /// Mounts bytes at an ASCII path on C: with explicit read/write access.
    /// Paths are case-insensitive; relative paths start at the drive root.
    /// Files persist across program loads.
    /// Replacing a mounted path leaves existing open handles on the previous bytes.
    pub fn mount_file(
        &mut self,
        path: &str,
        contents: Vec<u8>,
        access: FileAccess,
    ) -> Result<(), MountError> {
        self.runtime.state_mut().files.mount(path, contents, access)
    }

    /// Copies a file's current contents for the host to save or export.
    /// Returns None for invalid paths or files that do not exist.
    pub fn read_file(&self, path: &str) -> Option<Vec<u8>> {
        self.runtime.state().files.read_file(path)
    }

    pub fn runtime(&mut self) -> &mut Runtime<B> {
        &mut self.runtime
    }

    /// None until a DOS service terminates the process or reports a service error.
    pub fn exit_status(&self) -> Option<Result<u8, ServiceError>> {
        self.runtime.state().exit_status
    }

    /// Drains combined stdout/stderr bytes without interpreting their encoding.
    pub fn take_output(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.runtime.state_mut().output)
    }
}
