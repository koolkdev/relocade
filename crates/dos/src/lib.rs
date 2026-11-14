//! A minimal DOS environment for one real-mode COM or MZ program.
//!
//! COM programs start at PSP:0100. They receive the
//! available conventional memory; the image and initial stack use its first 64 KiB.
//! MZ executables use their header's memory request, relocation table and CS:IP/SS:SP.
//! Both formats receive a separate environment block through PSP:002C, containing
//! the supplied variables and executable path. Its storage comes from conventional RAM.
//! Their PSP saves INT 22h/23h/24h at 000Ah/000Eh/0012h. Termination restores those
//! vectors from guest RAM, including guest edits, while preserving unrelated IVT
//! entries. The root process returns to its host caller; it does not execute
//! INT 22h. Child EXEC and critical-error dispatch are not implemented.
//! DOS initializes INT 2Ah through 3Fh to empty handlers that preserve registers
//! and FLAGS. Installed services replace their vectors; the built-in mouse driver
//! takes INT 33h. INT 2Dh/2Fh therefore begin as empty multiplex chains.
//! XMS (4300h), DPMI (1687h), MSCDEX (1500h with BX=0), ANSI (1A00h),
//! SHARE (1000h), Windows (1600h), IPX (7A00h) and AMIS installation probes
//! therefore report absence. A guest resident service can replace either vector
//! and chain unclaimed calls to its saved target. No driver entry point or
//! installed signature is advertised by the empty chain.
//! INT 25h/26h report unavailable raw-volume access (AX=0201h, carry set,
//! interrupts enabled). Their far return leaves the original FLAGS word on the
//! guest stack; the caller must discard it. Mounted filesystems expose files,
//! not raw sectors, so these calls do not transfer data.
//! BIOS INT 1Ah/AH=00h reads the BIOS data area's clock. PIT channel 0 raises
//! IRQ0 about 18.2 times per second by default; guest programming can change its
//! rate. The BIOS handler advances the clock on each delivered interrupt.
//! INT 21h/AH=2Ah and AH=2Ch read the host's local system date and time,
//! including the weekday and hundredths of a second. These calls preserve flags
//! and do not consume the BIOS midnight counter or depend on delivered IRQ0 ticks.
//! AH=2Bh/2Dh set the DOS date/time through a session-local offset, preserving
//! the other component and leaving the host clock unchanged. Dates must be valid
//! within 1980–2099; time fields accept 0–23 hours, 0–59 minutes/seconds and
//! 0–99 hundredths. AL=00h indicates success, AL=FFh invalid input; flags are preserved.
//! The offset survives program launches. DOS time keeps following host wall time,
//! including clock corrections and local timezone changes, independently of CPU pacing.
//! INT 21h supports AH=09h string output, AH=40h stdout/stderr writes and
//! AH=4Ch termination. INT 20h also exits,
//! including a COM program's initial near RET. CON output is buffered as guest
//! bytes and also updates the VGA text display through BIOS services, including input echo.
//! Graphics-mode console bytes remain buffered but do not draw glyphs yet.
//! AH=06h polls stdin without echo or writes one byte to stdout.
//! AH=01h waits for a character and echoes it through stdout; AH=07h/08h omit echo.
//! AH=01h/02h/08h/09h/0Ah/0Bh and cooked console handle I/O check Ctrl-C.
//! DOS's INT 1Bh converts physical Ctrl-Break to console Ctrl-C. CON skips the
//! BIOS's 0000h dummy keys; extended keys still produce zero followed by a scan byte.
//! A top-level checked call prints ^C and invokes the live INT 23h with the
//! original user registers and stack and InDOS clear. The default handler terminates.
//! Operations await that callback directly, then retry character input or restart
//! their line/string from the beginning. Continuing handlers preserve registers.
//! IRET and RETF 2 continue regardless of carry; ordinary RETF uses carry to
//! choose continuation or termination.
//! AH=33h/AL=00h/01h/02h gets, sets or exchanges BREAK checking. BREAK ON extends
//! console checks to other INT 21h calls, even with redirected stdin. AH=06h/07h
//! bypass those checks; AH=33h itself bypasses InDOS and break checking.
//! AH=02h writes DL through stdout. Cooked console output expands tabs to
//! eight-column stops. [`Cpu::run`] awaits an empty input source and resumes
//! the pending read when input becomes available.
//! AH=0Bh checks for a break, then queries input availability. IOCTL AX=4406h
//! queries input without consuming it; AX=4407h reports
//! output readiness. Console status describes queued keys, not a buffered line.
//! AH=0Ah reads a line with echo and backspace editing into DS:DX's DOS buffer.
//! Its capacity includes the terminating CR; the returned count excludes it.
//! Queued complete lines return immediately; incomplete lines await further input.
//! Waiting character reads invoke the live INT 28h idle callback with InDOS set.
//! Empty AH=06h/0Bh polls and checked output status checks invoke it once and keep
//! their original status. Callbacks can use DOS functions above 0Ch; nested calls,
//! critical-error mode and AH=3Fh handle waits do not generate idle callbacks.
//! The default empty callback leaves input asleep until its source wakes it.
//! AH=0Ch flushes queued console input, then invokes AL=01h/06h/07h/08h/0Ah.
//! Other AL values only flush; redirected files and buffered handle lines are retained.
//! AH=3Fh on a console handle defaults to cooked mode: it reads an edited line
//! with CRLF and retains unread bytes for later console reads. Input echoes to
//! the console even when stdout is redirected. A leading Ctrl-Z sets EOF on
//! that open handle and its duplicates. Handle reads at EOF return zero until
//! EOF is cleared through AH=40h or AX=4401h, or a program load replaces the handles.
//! AX=4401h switches device handles between cooked and raw mode. Raw transfers
//! report the driver's actual byte count. Built-in CON reads await the requested
//! count without editing, echo, CRLF insertion or interpreting Ctrl-Z as EOF.
//! Raw writes preserve tabs and control bytes.
//! Duplicates share mode and EOF changes; separate opens
//! keep their own state. Switching modes preserves the buffered cooked-line remainder.
//! Dropping a run waiter preserves an active read and its local state. A later
//! [`Cpu::run`] resumes that read. Dropping the DOS session drops its pending call.
//! Character and buffered reads stay blocked at redirected file EOF; host file
//! growth does not yet wake them. AH=3Fh file reads return zero at EOF.
//! The host writes guest-encoded bytes through [`Dos::stdin`] and drains
//! [`Dos::stdout`] and [`Dos::stderr`] using `std::io` traits. Owned stream handles
//! remain usable while an asynchronous run waits for input.
//! [`Dos::keyboard`] also accepts translated keystrokes and modifier state.
//! BIOS INT 16h provides legacy AH=00h/01h/02h and enhanced AH=10h/11h/12h
//! interfaces to that same keyboard; the DOS console uses enhanced reads.
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
//! ASCII pathname components use DOS 8.3 translation: names and extensions are
//! uppercased, truncated to eight and three bytes, and compared as padded fields.
//! Embedded spaces are significant; trailing field spaces are padding.
//! Files, directories, device lookup and search masks share this translation.
//! AH=1Ah/2Fh select/query the DTA; AH=4Eh/4Fh find matching 8.3 names.
//! Search dates are initially fixed at 1980-01-01; long-name aliases are not generated.
//! AH=30h reports DOS 5.00; AH=25h/35h set/query the real interrupt vector table.
//! AH=34h returns ES:BX pointing to the resident InDOS byte. It counts active
//! INT 21h calls, including suspended input and nested services, and returns to
//! zero when those calls complete or are abandoned. Its address survives loads.
//! AH=71h reports long-filename services unavailable, allowing 8.3 fallback.
//! DOS 5 null functions AH=18h/1Dh/1Eh/20h/61h/6Bh return AL=0 without changing
//! FLAGS. Unknown functions above AH=6Ch do the same before DOS entry, except
//! AH=71h retains its explicit unavailable response. Known DOS services that
//! are not implemented still report [`ServiceError::UnsupportedFunction`] to
//! the host; their different return conventions need individual implementations.
//! AH=42h seeks within those files relative to their start, current position or end.
//! AH=43h queries or changes read-only, hidden, system and archive attributes.
//! Closing a written file handle restores the archive attribute.
//! AH=68h/6Ah commits that metadata while preserving the open handle and its cursor.
//! Creation and modified-file commit/close use DOS's session clock at two-second
//! precision.
//! Directory searches return the committed timestamps. Supplied files start with
//! the mount time, and timestamps persist across program launches with the files.
//! AH=41h deletes files after read-only and open-handle checks.
//! AH=45h/46h duplicate handles, sharing file positions and allowing redirection.
//! AH=09h string output follows the current stdout handle, just like AH=40h.
//! AX=4400h reports file/device information for the object behind a handle.
//! [`Dos::new`] prepares the DOS session. Start a process with [`Dos::launch_com`]
//! or [`Dos::launch_mz`], passing startup inputs in [`LoadOptions`].
//! Construct DOS inside an [`relocade_machine::ExecutionWorker`] session.
//! [`Dos::new`] creates a [`relocade_pc::Pc`] and installs DOS on it.
//! [`Dos::with_configuration`] selects its hardware using the PC's [`Configuration`].
//! [`Dos::from_pc`] installs DOS onto an existing BIOS-initialized PC.
//! [`Dos::initial_environment`] supplies hardware defaults for a caller-owned
//! environment, passed through [`LoadOptions`] when launching programs.
//! With the `audio` feature, `Dos::attach_audio` connects the PC's mixed output.
//! Guest AdLib ports (388h/389h) and PC-speaker ports share that output and work
//! independently of DOS services. Attach before starting the first guest program.
//! DOS opens no platform output; the PC submits PCM on the execution worker.
//! [`Dos::run`] returns the current program's exit code; devices keep running while
//! the main session awaits input or another program. [`Dos::frames`] exposes
//! captured display state across launches. The caller chooses the rendering implementation
//! and its worker, scheduling and presentation lifetime.
//! Direct CPU control remains available through [`Dos::cpu`].
//! The [native/browser fixture](https://github.com/koolkdev/relocade/blob/main/tests/browser/src/dos.rs)
//! demonstrates persistent execution and an independent renderer worker.
//!
//! ```no_run
//! # async fn example(runner: relocade_machine::Runner) -> Result<(), Box<dyn std::error::Error>>
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
//! assert_eq!(dos.run().await?, 7);
//! assert_eq!(process.status(), ProcessStatus::Exited(Ok(7)));
//! let mut output = Vec::new();
//! dos.stdout().read_to_end(&mut output)?;
//! # let _ = output;
//! # Ok(())
//! # }
//! ```
#![forbid(unsafe_code)]

mod com;
mod console;
pub mod devices;
mod error;
mod execution;
mod files;
pub mod interrupts;
mod load;
mod memory;
mod mouse;
mod mz;
mod path;
mod process;
mod services;
mod state;
mod stdio;

pub use console::NativeConsole;
pub use error::{DosError, LaunchError, LoadError, MountError, ServiceError};
pub use execution::RunError;
pub use files::FileAccess;
pub use load::LoadOptions;
pub use process::{Process, ProcessStatus};
use relocade_machine::{real_mode_interrupt::Interrupts, Cpu, CpuContext, Runner};
pub use relocade_pc::{configuration, video_card, Configuration, SoundCard, VideoCard};
use relocade_pc::{
    sound_card::{Settings as SoundCardSettings, SoundBlasterModel},
    Pc,
};
use relocade_pc_speaker::Speaker;
use state::DosState;
use std::{cell::RefCell, rc::Rc};

/// A DOS session with persistent CPU, memory, filesystems and console resources.
/// Devices share the runner's clock; the default uses the host timer service.
pub struct Dos {
    state: Rc<DosState>,
    pc: Pc,
}

impl Dos {
    /// Creates a BIOS-initialized PC and installs DOS services on it.
    /// Launch a program before executing DOS services.
    pub async fn new(runner: Runner) -> relocade_machine::Result<Self> {
        Self::with_configuration(runner, Configuration::default()).await
    }

    /// Creates a PC with the selected hardware and installs DOS services on it.
    /// Hardware selection persists across process launches and does not depend
    /// on attaching host audio output.
    pub async fn with_configuration(
        runner: Runner,
        configuration: Configuration,
    ) -> relocade_machine::Result<Self> {
        Self::from_pc(Pc::with_configuration(runner, configuration).await?).await
    }

    /// Installs the DOS allocator, console devices, filesystem and interrupt
    /// handlers on an existing PC. Hardware and queued input are preserved.
    /// Conventional memory from the first MCB onward becomes the DOS process arena.
    /// Launch a program before executing DOS services.
    ///
    /// # Panics
    /// The CPU must be stopped with no pending or suspended host services.
    pub async fn from_pc(mut pc: Pc) -> relocade_machine::Result<Self> {
        let clock = pc.clock().clone();
        let keyboard = pc.keyboard();
        let mouse = mouse::Mouse::new(pc.video_card().framebuffer_access());
        let cpu = pc.cpu();
        let memory = cpu.memory();
        let io = cpu.context().io.clone();
        let exit = cpu.exit_handle();
        let mut interrupts = Interrupts::new(cpu);
        let state = Rc::new(DosState::new(
            memory.clone().with_io(io),
            keyboard,
            exit,
            mouse,
        ));
        memory::reset(&mut memory.view());
        interrupts::register_interrupts(&mut interrupts, Rc::clone(&state));
        mouse::register(&mut interrupts, Rc::clone(&state), clock);
        interrupts.install().await?;
        Ok(Self { state, pc })
    }

    /// Creates environment defaults from the installed hardware.
    /// Call once when preparing a launcher or shell, then keep and edit the returned
    /// entries. Pass their byte slices through [`LoadOptions::environment`] at launch;
    /// launches copy the supplied environment without restoring removed defaults.
    pub fn initial_environment(&self) -> Vec<Vec<u8>> {
        let Some(SoundCardSettings::SoundBlaster(settings)) =
            self.pc.sound_card().map(|card| card.settings())
        else {
            return Vec::new();
        };
        let model = match settings.model {
            SoundBlasterModel::SoundBlaster2 => 3,
        };
        vec![format!(
            "BLASTER=A{:X} I{} D{} T{}",
            settings.base_port, settings.irq, settings.dma_channel, model
        )
        .into_bytes()]
    }

    /// Accesses the session's speaker on its execution owner.
    /// Hardware state persists across process launches.
    pub fn speaker(&self) -> Rc<RefCell<Speaker>> {
        self.pc.speaker()
    }

    /// Attaches the PC's mixed audio output once for this execution session.
    /// Output persists across DOS process launches and ends with the session.
    /// Attach before guest execution to retain the full AdLib note timeline.
    #[cfg(feature = "audio")]
    pub async fn attach_audio(
        &mut self,
        config: cpal::StreamConfig,
        samples: async_channel::Sender<Vec<f32>>,
    ) -> Result<(), String> {
        self.pc.attach_audio(config, samples).await
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
        let psp_segment = load(&mut self.pc.cpu().context())?;
        self.state.start_process(psp_segment);
        Ok(())
    }

    /// Mounts a filesystem at A: through Z:, replacing any previous mount.
    /// Filesystem paths must use uppercase, unpadded ASCII 8.3 names and '/' separators.
    /// Entries with other spellings are not assigned short-name aliases.
    /// Files larger than u32::MAX bytes cannot be opened or listed by DOS.
    /// Resets that drive's current directory to its root and discards its DOS
    /// attributes and searches; entries start with the current DOS timestamp.
    /// Open handles continue to access their original files.
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
    /// Paths use DOS 8.3 translation, including case folding and field truncation;
    /// relative paths start at the drive root.
    /// An omitted drive defaults to C:, independently of the guest's current drive.
    /// Files persist across program loads.
    /// The file starts with the current DOS timestamp.
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
    /// Like mount_file, paths use DOS 8.3 translation, start at the root and default to C:.
    /// Returns None for invalid paths or files that do not exist.
    pub fn read_file(&self, path: &str) -> Option<Vec<u8>> {
        self.state.filesystem().read_file(path)
    }

    /// Returns the session's CPU.
    /// Executing a DOS hook before launch or after process termination is a
    /// programming error and panics. BIOS and custom hooks do not require a DOS process.
    pub fn cpu(&mut self) -> &mut Cpu {
        self.pc.cpu()
    }

    /// Returns the current process, or None before the first successful launch.
    /// The process remains current after termination until another launch succeeds.
    /// Retained handles keep their identity and status while the CPU runs and after
    /// a later process starts. A failed launch leaves this handle unchanged.
    pub fn process(&self) -> Option<Rc<Process>> {
        self.state.process()
    }

    /// Returns an owned console input writer. Bytes use the guest's encoding.
    /// Writes store bytes with scan code zero in the BIOS ring and wake a pending
    /// reader. This is nonblocking: a full ring produces a short write or
    /// `WouldBlock`. Retry only the unwritten suffix after running the guest;
    /// `write_all` may accept a prefix before returning `WouldBlock`.
    /// Flush is a no-op. The writer can be cloned while `run` is pending.
    /// Redirected stdin reads its file instead. Buffered input and output persist
    /// across launches; existing stream handles remain connected to the console.
    ///
    /// ```no_run
    /// # async fn example(runner: relocade_machine::Runner) -> Result<(), Box<dyn std::error::Error>>
    /// # {
    /// use relocade_dos::ProcessStatus;
    /// use std::io::Write;
    ///
    /// let mut dos = relocade_dos::Dos::new(runner).await?;
    /// dos.launch_com(&[0xb4, 7, 0xcd, 0x21, 0xb4, 0x4c, 0xcd, 0x21], Default::default())?;
    /// let process = dos.process().unwrap();
    /// let mut input = dos.stdin();
    /// // An event handler can retain this writer while the cpu waits.
    /// let run = dos.cpu().run();
    /// input.write_all(b"y")?;
    /// run.await;
    /// assert_eq!(process.status(), ProcessStatus::Exited(Ok(b'y')));
    /// # Ok(())
    /// # }
    /// ```
    pub fn stdin(&self) -> impl std::io::Write + Clone + 'static {
        self.state.stdio().input.clone()
    }

    /// Returns the BIOS keyboard ring and modifier state shared with DOS reads.
    /// `store_key` uses the same finite insertion as INT 16h/AH=05h; physical events
    /// belong on [`Self::keyboard_input`]. Retain this handle while input is awaited.
    ///
    /// ```no_run
    /// # fn example(dos: &relocade_dos::Dos) {
    /// assert!(dos.keyboard().store_key(relocade_bios::KeyStroke {
    ///     character: b'a',
    ///     scan_code: 0x1e,
    /// }));
    /// # }
    /// ```
    pub fn keyboard(&self) -> relocade_bios::Keyboard {
        self.state.stdio().input.clone()
    }

    /// Thread-safe physical keyboard input for guest execution. Events reach the
    /// AT controller and IRQ1; guest INT 09h handlers decide how to consume them.
    pub fn keyboard_input(&self) -> relocade_frontend::keyboard::KeyboardInput {
        self.pc.keyboard_input()
    }

    /// Relative motion, buttons and focus-loss events for the PS/2 mouse.
    /// DOS installs an INT 33h driver using BIOS INT 15h/C2h packet callbacks.
    /// Guests may instead replace the vectors and own the hardware directly.
    pub fn mouse_input(&self) -> relocade_frontend::mouse::MouseInput {
        self.pc.mouse_input()
    }

    /// Opens a native prompt over application-owned physical input and BIOS video.
    /// It shares line editing and cooked output with DOS, but executes no guest
    /// code and uses neither DOS handles nor the guest keyboard queue. Drop the
    /// console before launching or resuming a guest program.
    ///
    /// # Panics
    /// Panics if a guest host service is still pending. Open the prompt at startup
    /// or after guest execution has completed, rather than after cancelling a read.
    pub fn native_console(
        &mut self,
        events: relocade_frontend::keyboard::KeyboardEvents,
    ) -> NativeConsole<'_> {
        let memory = self.pc.cpu().memory();
        let io = self.pc.cpu().context().io.clone();
        NativeConsole::new(memory.with_io(io), self.state.stdio(), events)
    }

    /// Installs a driver as a named character device at the front of DOS's chain.
    /// New lookups and process launches resolve the new device; retained devices
    /// and open handles keep their original driver binding. Each call installs
    /// a distinct device, even for the same name.
    ///
    /// # Panics
    /// Panics if the driver attributes do not identify a character device.
    pub fn install_driver(&self, driver: impl devices::CharacterDriver + 'static) {
        self.state.devices().install(driver);
    }

    /// Resolves a character device by its unpadded DOS name, ignoring ASCII case.
    /// The returned device keeps its driver binding after another installation.
    /// Its I/O calls the driver directly, without DOS line editing or cooked output.
    pub fn device(&self, name: &[u8]) -> Option<Rc<devices::CharacterDevice>> {
        self.state.devices().find(name)
    }

    /// Snapshot queue for one graphics consumer, reusable across program launches.
    /// Capture continues independently of application redraws.
    pub fn frames(&self) -> async_channel::Receiver<relocade_vga::DisplaySnapshot> {
        self.pc.frames()
    }

    /// The installed card's frame delivery and native framebuffer access.
    pub fn video_card(&self) -> &video_card::VideoCard {
        self.pc.video_card()
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

#[cfg(test)]
#[path = "../tests/common/runner.rs"]
mod test_runner;
