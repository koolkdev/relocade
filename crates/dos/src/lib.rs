//! A minimal DOS environment for one real-mode COM or MZ program.
//!
//! COM programs start at PSP:0100. They receive the
//! available conventional memory; the image and initial stack use its first 64 KiB.
//! MZ executables use their header's memory request, relocation table and CS:IP/SS:SP.
//! Both formats receive a separate environment block through PSP:002C, containing
//! the supplied variables and executable path. Its storage comes from conventional RAM.
//! BIOS INT 1Ah/AH=00h reads the BIOS data area's clock; ticks initially remain zero
//! until the embedding updates them.
//! INT 21h supports AH=09h string output, AH=40h stdout/stderr writes and
//! AH=4Ch termination. INT 20h also exits,
//! including a COM program's initial near RET. Output is buffered as guest bytes.
//! AH=06h polls stdin without echo or writes one byte to stdout.
//! AH=01h waits for a character and echoes it through stdout; AH=07h/08h omit echo.
//! AH=02h writes DL through stdout. Cooked console output expands tabs to
//! eight-column stops. An empty input source returns
//! [`relocade_x86::ExecutionExit::Deferred`] from a synchronous run. Use
//! [`Runtime::run_async`] to await input and complete the pending read.
//! AH=0Bh and IOCTL AX=4406h query input without consuming it; AX=4407h reports
//! output readiness. Console status describes queued keys, not a buffered line.
//! AH=0Ah reads a line with echo and backspace editing into DS:DX's DOS buffer.
//! Its capacity includes the terminating CR; the returned count excludes it.
//! Queued complete lines return immediately; incomplete lines await further input.
//! AH=0Ch flushes queued console input, then invokes AL=01h/06h/07h/08h/0Ah.
//! Other AL values only flush; redirected files and buffered handle lines are retained.
//! AH=3Fh on a console input handle reads an edited line with CRLF, retaining
//! unread bytes for later reads through any duplicate of the console handle.
//! Console handle input echoes to the console even when stdout is redirected.
//! Leading Ctrl-Z sets EOF on that open console handle and its duplicates.
//! Further handle reads return zero until AH=40h through that handle clears EOF
//! or a program load replaces the handles.
//! Keep the same run future alive while waiting; dropping an active read
//! cancels it and requires reloading the program before further execution.
//! Character and buffered reads stay blocked at redirected file EOF; host file
//! growth does not yet wake them. AH=3Fh file reads return zero at EOF.
//! The host writes guest-encoded bytes through [`Dos::stdin`] and drains
//! [`Dos::stdout`] and [`Dos::stderr`] using `std::io` traits. Owned stream handles
//! remain usable while an asynchronous run waits for input.
//! AH=48h/49h/4Ah allocate, free and resize conventional-memory blocks.
//! [`Dos::mount`] supplies a filesystem at a drive letter; C: starts empty.
//! [`Dos::mount_file`] supplies files for AH=3Dh/3Eh/3Fh/40h open/close/read/write.
//! AH=3Ch creates or truncates files in existing directories.
//! AH=5Bh creates a file only if its name is unused, preserving existing files.
//! AH=5Ah creates a uniquely named temporary file; the caller owns its deletion.
//! AH=56h renames or moves closed files on the same drive without replacing a destination.
//! AH=0Eh/19h select/query the current drive; AH=3Bh/47h change/query a drive's directory.
//! AH=39h/3Ah create directories or remove empty ones, excluding root and current.
//! Guest relative paths use the selected drive's directory; both persist across loads.
//! AH=1Ah/2Fh select/query the DTA; AH=4Eh/4Fh find matching 8.3 names.
//! Search dates are initially fixed at 1980-01-01; long-name aliases are not generated.
//! AH=30h reports DOS 5.00; AH=25h/35h set/query the real interrupt vector table.
//! AH=71h reports long-filename services unavailable, allowing 8.3 fallback.
//! AH=42h seeks within those files relative to their start, current position or end.
//! AH=43h queries or changes read-only, hidden, system and archive attributes.
//! Closing a written file handle restores the archive attribute.
//! AH=68h/6Ah commits that metadata while preserving the open handle and its cursor.
//! AH=41h deletes files after read-only and open-handle checks.
//! AH=45h/46h duplicate handles, sharing file positions and allowing redirection.
//! AH=09h string output follows the current stdout handle, just like AH=40h.
//! AX=4400h reports file/device information for the object behind a handle.
//! [`Dos::new`] prepares the DOS environment. Load a program with [`Dos::load_com`]
//! or [`Dos::load_mz`], passing startup inputs in [`LoadOptions`].
//! The host chooses the Wasm backend and controls execution through
//! [`Dos::runtime`]. Create the backend with its `with_state` constructor and
//! `RuntimeState::new(DosState::default())`.
//!
//! ```no_run
//! # async fn example<B>(backend: B) -> Result<(), Box<dyn std::error::Error>>
//! # where B: relocade_runtime::RuntimeBackend<State = relocade_runtime::RuntimeState<relocade_dos::DosState>>
//! # {
//! use std::io::Read;
//!
//! let mut dos = relocade_dos::Dos::new(backend).await?;
//! let mut disk = relocade_vfs::MemoryFileSystem::new();
//! disk.replace_file("README.TXT", b"mounted on D:".to_vec())?;
//! dos.mount('D', disk)?;
//! dos.load_com(&[0xb8, 7, 0x4c, 0xcd, 0x21], Default::default())?;
//! assert_eq!(dos.runtime().run(u64::MAX)?, relocade_x86::ExecutionExit::HostReturn);
//! assert_eq!(dos.exit_status(), Some(Ok(7)));
//! let mut output = Vec::new();
//! dos.stdout().read_to_end(&mut output)?;
//! # let _ = output;
//! # Ok(())
//! # }
//! ```
#![forbid(unsafe_code)]

mod com;
mod error;
mod files;
mod interrupt;
mod kernel;
mod load;
mod memory;
mod mz;
mod path;
mod process;
mod services;
mod stdio;

pub use error::{LoadError, MountError, ServiceError};
pub use files::FileAccess;
pub use kernel::DosState;
pub use load::LoadOptions;
use relocade_runtime::{Runtime, RuntimeBackend, RuntimeState};

/// A DOS environment and its runtime, with space for one loaded program.
pub struct Dos<B: RuntimeBackend<State = RuntimeState<DosState>>> {
    runtime: Runtime<B>,
}

impl<B: RuntimeBackend<State = RuntimeState<DosState>>> Dos<B> {
    /// Prepares the memory map, interrupt vectors and DOS service hooks.
    /// Load a program before entering guest execution.
    pub async fn new(backend: B) -> relocade_runtime::Result<Self> {
        let mut runtime = Runtime::new(backend, kernel::configuration()).await?;
        kernel::initialize(&mut runtime.machine().memory());
        relocade_bios::initialize(&mut runtime.machine().memory());
        runtime
            .install_hooks(kernel::hooks().into_iter().chain(relocade_bios::hooks()))
            .await?;
        Ok(Self { runtime })
    }

    /// Loads a COM image of 1..=65,278 bytes and initializes its PSP, stack and CPU.
    /// Replaces the process allocation and clears buffered I/O and exit status.
    /// Invalid images or options leave the existing program and its state unchanged.
    pub fn load_com(&mut self, image: &[u8], options: LoadOptions<'_>) -> Result<(), LoadError> {
        let psp_segment = com::load(&mut self.runtime.machine(), image, &options)?;
        self.runtime.cancel_pending_call();
        self.runtime.state_mut().reset_process(psp_segment);
        Ok(())
    }

    /// Loads a DOS MZ executable, applies relocations and initializes its PSP and CPU.
    /// Replaces the root process and clears buffered I/O and exit status.
    /// Invalid images or options leave the existing program and its state unchanged.
    /// Overlay loading is not supported.
    pub fn load_mz(&mut self, image: &[u8], options: LoadOptions<'_>) -> Result<(), LoadError> {
        let psp_segment = mz::load(&mut self.runtime.machine(), image, &options)?;
        self.runtime.cancel_pending_call();
        self.runtime.state_mut().reset_process(psp_segment);
        Ok(())
    }

    /// Mounts a filesystem at A: through Z:, replacing any previous mount.
    /// Filesystem paths use uppercase ASCII names and '/' separators for DOS.
    /// Files larger than u32::MAX bytes cannot be opened or listed by DOS.
    /// Resets that drive's current directory to its root and discards its DOS
    /// attributes and searches. Open handles continue to access their original files.
    pub fn mount(
        &mut self,
        letter: char,
        filesystem: relocade_vfs::MemoryFileSystem,
    ) -> std::io::Result<()> {
        self.runtime.state_mut().files.mount(letter, filesystem)
    }

    /// Supplies bytes at an ASCII path on a mounted drive with an initial read-only attribute.
    /// DOS AH=43h may change that attribute; existing handles keep their access rights.
    /// Paths are case-insensitive; relative paths start at the drive root.
    /// An omitted drive defaults to C:, independently of the guest's current drive.
    /// Files persist across program loads.
    /// Replacing a mounted path leaves existing open handles on the previous bytes.
    pub fn mount_file(
        &mut self,
        path: &str,
        contents: Vec<u8>,
        access: FileAccess,
    ) -> Result<(), MountError> {
        self.runtime
            .state_mut()
            .files
            .mount_file(path, contents, access)
    }

    /// Copies a file's current contents for the host to save or export.
    /// Like mount_file, relative paths start at the root and default to C:.
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

    /// Returns an owned console input writer. Bytes use the guest's encoding.
    /// Writes enqueue bytes immediately and wake a pending console read; flush
    /// is a no-op. The writer can be cloned and used while `run_async` is pending.
    /// Redirected stdin reads its file instead. Loading a program clears buffered
    /// input and output; existing stream handles remain connected to the console.
    ///
    /// ```no_run
    /// # async fn example<B>(backend: B) -> Result<(), Box<dyn std::error::Error>>
    /// # where B: relocade_runtime::RuntimeBackend<State = relocade_runtime::RuntimeState<relocade_dos::DosState>>
    /// # {
    /// use std::io::Write;
    ///
    /// let mut dos = relocade_dos::Dos::new(backend).await?;
    /// dos.load_com(&[0xb4, 7, 0xcd, 0x21, 0xb4, 0x4c, 0xcd, 0x21], Default::default())?;
    /// let mut input = dos.stdin();
    /// // An event handler can retain this writer while the runtime waits.
    /// let run = dos.runtime().run_async(100);
    /// input.write_all(b"y")?;
    /// run.await?;
    /// assert_eq!(dos.exit_status(), Some(Ok(b'y')));
    /// # Ok(())
    /// # }
    /// ```
    pub fn stdin(&self) -> impl std::io::Write + Clone + 'static {
        self.runtime.state().files.stdio.input.clone()
    }

    /// Returns an owned reader that drains currently buffered standard output.
    /// An empty buffer returns zero, even if the guest may produce more later.
    /// It can be used while `run_async` waits. DOS redirection follows the handle's
    /// destination, so bytes redirected to a file do not appear here.
    pub fn stdout(&self) -> impl std::io::Read + Clone + 'static {
        self.runtime
            .state()
            .files
            .stdio
            .output(stdio::OutputStream::Stdout)
            .clone()
    }

    /// Returns an owned reader that drains standard error, separately from stdout.
    /// Like stdout, an empty buffer returns zero and reads do not block.
    pub fn stderr(&self) -> impl std::io::Read + Clone + 'static {
        self.runtime
            .state()
            .files
            .stdio
            .output(stdio::OutputStream::Stderr)
            .clone()
    }
}
