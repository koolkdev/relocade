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
//! including a COM program's initial near RET. CON output is buffered as guest
//! bytes and also updates the VGA text display through BIOS services, including input echo.
//! Graphics-mode console bytes remain buffered but do not draw glyphs yet.
//! AH=06h polls stdin without echo or writes one byte to stdout.
//! AH=01h waits for a character and echoes it through stdout; AH=07h/08h omit echo.
//! AH=02h writes DL through stdout. Cooked console output expands tabs to
//! eight-column stops. [`Cpu::run`] awaits an empty input source and resumes
//! the pending read when input becomes available.
//! AH=0Bh and IOCTL AX=4406h query input without consuming it; AX=4407h reports
//! output readiness. Console status describes queued keys, not a buffered line.
//! AH=0Ah reads a line with echo and backspace editing into DS:DX's DOS buffer.
//! Its capacity includes the terminating CR; the returned count excludes it.
//! Queued complete lines return immediately; incomplete lines await further input.
//! AH=0Ch flushes queued console input, then invokes AL=01h/06h/07h/08h/0Ah.
//! Other AL values only flush; redirected files and buffered handle lines are retained.
//! AH=3Fh on a console handle defaults to cooked mode: it reads an edited line
//! with CRLF and retains unread bytes for later console reads. Input echoes to
//! the console even when stdout is redirected. A leading Ctrl-Z sets EOF on
//! that open handle and its duplicates. Handle reads at EOF return zero until
//! EOF is cleared through AH=40h or AX=4401h, or a program load replaces the handles.
//! AX=4401h switches device handles between cooked and raw mode. Raw AH=3Fh reads
//! await the requested byte count, limited by the segment boundary, without editing,
//! echo, CRLF insertion or interpreting Ctrl-Z as EOF. Raw AH=40h writes preserve
//! tabs and control bytes. Duplicates share mode and EOF changes; separate opens
//! keep their own state. Switching modes preserves the buffered cooked-line remainder.
//! Dropping a run waiter preserves an active read and its local state. A later
//! [`Cpu::run`] resumes that read. Dropping the DOS session drops its pending call.
//! Character and buffered reads stay blocked at redirected file EOF; host file
//! growth does not yet wake them. AH=3Fh file reads return zero at EOF.
//! The host writes guest-encoded bytes through [`Dos::stdin`] and drains
//! [`Dos::stdout`] and [`Dos::stderr`] using `std::io` traits. Owned stream handles
//! remain usable while an asynchronous run waits for input.
//! [`Dos::keyboard`] also accepts translated keystrokes and modifier state.
//! BIOS INT 16h/AH=00h/01h/02h consumes, peeks and queries that same keyboard.
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
//! [`Dos::new`] prepares the DOS session. Start a process with [`Dos::launch_com`]
//! or [`Dos::launch_mz`], passing startup inputs in [`LoadOptions`].
//! Construct DOS inside [`relocade_machine::Runner::run`] or an execution worker.
//! [`Dos::new`] registers VGA with that runner and shares its clock. [`Dos::run`]
//! returns the current program's exit code; registered devices keep running while
//! the main session awaits input or another program. [`Dos::frame_source`] exposes
//! captured display state across launches. The caller chooses the rendering implementation
//! and its worker, scheduling and presentation lifetime.
//! Direct CPU control remains available through [`Dos::cpu`].
//! The [native/browser fixture](https://github.com/koolkdev/relocade/blob/main/tests/browser/src/dos.rs)
//! demonstrates persistent execution and an independent renderer worker.
//!
//! ```no_run
//! # async fn example(runner: &relocade_machine::Runner) -> Result<(), Box<dyn std::error::Error>>
//! # {
//! use relocade_dos::ProcessStatus;
//! use std::io::Read;
//!
//! let mut dos = relocade_dos::Dos::new(runner).await?;
//! let mut disk = relocade_vfs::MemoryFileSystem::new();
//! disk.replace_file("README.TXT", b"mounted on D:".to_vec())?;
//! dos.mount('D', disk)?;
//! dos.launch_com(&[0xb8, 7, 0x4c, 0xcd, 0x21], Default::default())?;
//! let process = dos.process().unwrap();
//! assert_eq!(dos.run(runner).await?, 7);
//! assert_eq!(process.status(), ProcessStatus::Exited(Ok(7)));
//! let mut output = Vec::new();
//! dos.stdout().read_to_end(&mut output)?;
//! # let _ = output;
//! # Ok(())
//! # }
//! ```
#![forbid(unsafe_code)]

mod com;
mod error;
mod execution;
mod files;
mod interrupt;
mod kernel;
mod load;
mod memory;
mod mz;
mod path;
mod process;
mod services;
mod state;
mod stdio;

pub use error::{LaunchError, LoadError, MountError, ServiceError};
pub use execution::RunError;
pub use files::FileAccess;
pub use load::LoadOptions;
pub use process::{Process, ProcessStatus};
use relocade_machine::{Cpu, CpuContext, IoBus, Memory, Runner};
use relocade_vga::Vga;
use state::DosState;
use std::{cell::RefCell, rc::Rc};

/// A DOS session with persistent CPU, memory, filesystems and console resources.
pub struct Dos {
    state: Rc<DosState>,
    cpu: Cpu,
    vga: Rc<RefCell<Vga>>,
}

impl Dos {
    /// Prepares the memory map and service hooks, and registers VGA with the runner.
    /// Launch a program before executing DOS services.
    pub async fn new(runner: &Runner) -> relocade_machine::Result<Self> {
        let vga = Rc::new(RefCell::new(Vga::with_clock(Rc::new(
            runner.clock().clone(),
        ))));
        let io = IoBus::new([runner.connect(
            vga.clone(),
            [relocade_vga::MEMORY_RANGE],
            relocade_vga::PORT_RANGES,
        )]);
        let memory = Memory::new(kernel::configuration())?;
        let state = Rc::new(DosState::new(memory.clone().with_io(io.clone())));
        let keyboard = state.stdio().input.clone();
        let mut cpu = Cpu::new(memory, kernel::cpu(), io, ()).await?;
        kernel::initialize(&mut cpu.context().memory);
        {
            let mut context = cpu.context();
            relocade_bios::initialize(&mut context.memory, context.io);
        }
        cpu.install_hooks(
            kernel::hooks(Rc::clone(&state))
                .into_iter()
                .chain(relocade_bios::hooks(keyboard)),
        )
        .await?;
        Ok(Self { state, cpu, vga })
    }

    /// Launches a COM image of 1..=65,278 bytes when no process is active.
    /// Initializes its PSP, environment, standard handles, stack and CPU entry state.
    /// Preserves RAM outside the PSP, environment, allocation metadata, image and
    /// initial return word.
    /// Successful launches create a new active process and preserve console I/O.
    /// Rejected launches leave the previous program and session state unchanged.
    pub fn launch_com(
        &mut self,
        image: &[u8],
        options: LoadOptions<'_>,
    ) -> Result<(), LaunchError> {
        self.launch(|execution| com::load(execution, image, &options))
    }

    /// Launches an MZ executable, applying relocations and its memory request.
    /// Like [`Self::launch_com`], requires the previous process to have terminated
    /// and any pending host service to have completed.
    /// Overlay loading is not supported.
    pub fn launch_mz(&mut self, image: &[u8], options: LoadOptions<'_>) -> Result<(), LaunchError> {
        self.launch(|execution| mz::load(execution, image, &options))
    }

    fn launch(
        &mut self,
        load: impl FnOnce(&mut CpuContext<'_>) -> Result<u16, LoadError>,
    ) -> Result<(), LaunchError> {
        // Reject before touching CPU state, including a pending input call.
        if self
            .state
            .process()
            .as_ref()
            .is_some_and(|process| process.status() == ProcessStatus::Active)
        {
            return Err(LaunchError::ProcessActive);
        }
        let psp_segment = load(&mut self.cpu.context())?;
        self.state.start_process(psp_segment);
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
        self.state.filesystem().mount(letter, filesystem)?;
        if let Some(process) = self.state.process() {
            if process.status() == ProcessStatus::Active {
                process
                    .files()
                    .invalidate_searches(letter.to_ascii_uppercase());
            }
        }
        Ok(())
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
        self.state.filesystem().mount_file(path, contents, access)
    }

    /// Copies a file's current contents for the host to save or export.
    /// Like mount_file, relative paths start at the root and default to C:.
    /// Returns None for invalid paths or files that do not exist.
    pub fn read_file(&self, path: &str) -> Option<Vec<u8>> {
        self.state.filesystem().read_file(path)
    }

    /// Returns the session's CPU.
    /// Executing a DOS hook before launch or after process termination is a
    /// programming error and panics. BIOS and custom hooks do not require a DOS process.
    pub fn cpu(&mut self) -> &mut Cpu {
        &mut self.cpu
    }

    /// Returns the current process, or None before the first successful launch.
    /// The process remains current after termination until another launch succeeds.
    /// Retained handles keep their identity and status while the CPU runs and after
    /// a later process starts. A failed launch leaves this handle unchanged.
    pub fn process(&self) -> Option<Rc<Process>> {
        self.state.process()
    }

    /// Returns an owned console input writer. Bytes use the guest's encoding.
    /// Writes enqueue bytes immediately and wake a pending console read; flush
    /// is a no-op. The writer can be cloned and used while `run` is pending.
    /// Redirected stdin reads its file instead. Buffered input and output persist
    /// across launches; existing stream handles remain connected to the console.
    ///
    /// ```no_run
    /// # async fn example(runner: &relocade_machine::Runner) -> Result<(), Box<dyn std::error::Error>>
    /// # {
    /// use relocade_dos::ProcessStatus;
    /// use std::io::Write;
    ///
    /// let mut dos = relocade_dos::Dos::new(runner).await?;
    /// dos.launch_com(&[0xb4, 7, 0xcd, 0x21, 0xb4, 0x4c, 0xcd, 0x21], Default::default())?;
    /// let process = dos.process().unwrap();
    /// let mut input = dos.stdin();
    /// // An event handler can retain this writer while the cpu waits.
    /// let run = dos.cpu().run(100);
    /// input.write_all(b"y")?;
    /// run.await;
    /// assert_eq!(process.status(), ProcessStatus::Exited(Ok(b'y')));
    /// # Ok(())
    /// # }
    /// ```
    pub fn stdin(&self) -> impl std::io::Write + Clone + 'static {
        self.state.stdio().input.clone()
    }

    /// Returns the input queue shared by BIOS INT 16h and DOS console reads.
    /// Host adapters supply translated characters, BIOS scan codes and shift state.
    /// The handle can be retained while `run` awaits keyboard input.
    ///
    /// ```no_run
    /// # fn example(dos: &relocade_dos::Dos) {
    /// dos.keyboard().push(relocade_bios::KeyStroke {
    ///     character: b'a',
    ///     scan_code: 0x1e,
    /// });
    /// # }
    /// ```
    pub fn keyboard(&self) -> relocade_bios::Keyboard {
        self.state.stdio().input.clone()
    }

    /// Immutable captured inputs, reusable across program launches. Rendering is
    /// caller-owned; capture continues independently of application redraws.
    pub fn frame_source(&self) -> relocade_vga::FrameSource {
        self.vga.borrow().frame_source()
    }

    /// Retains the execution-local device, including while the CPU awaits input.
    /// Give other workers the immutable captures from [`Dos::frame_source`].
    /// End device borrows before polling execution or dropping the session.
    pub fn vga(&self) -> Rc<RefCell<Vga>> {
        Rc::clone(&self.vga)
    }

    /// Returns an owned reader that drains currently buffered standard output.
    /// An empty buffer returns zero, even if the guest may produce more later.
    /// It can be used while `run` waits. DOS redirection follows the handle's
    /// destination, so bytes redirected to a file do not appear here.
    pub fn stdout(&self) -> impl std::io::Read + Clone + 'static {
        self.state
            .stdio()
            .output(stdio::OutputStream::Stdout)
            .clone()
    }

    /// Returns an owned reader that drains standard error, separately from stdout.
    /// Like stdout, an empty buffer returns zero and reads do not block.
    pub fn stderr(&self) -> impl std::io::Read + Clone + 'static {
        self.state
            .stdio()
            .output(stdio::OutputStream::Stderr)
            .clone()
    }
}
