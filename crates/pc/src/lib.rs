//! A BIOS-initialized PC, with hardware and firmware independent of DOS.
//!
//! [`Pc::new`] assembles the CPU, memory, DMA, PIC, PIT, PS/2 keyboard/mouse,
//! VGA, speaker and AdLib on an existing execution runner. It initializes BIOS
//! data and interrupt services. The caller supplies guest code and its entry state; this does not
//! execute a reset ROM, POST or a disk bootstrap.
//! [`Pc::with_configuration`] selects sound, video and optional parallel hardware.
//!
//! Devices retain their own emulation and async activities. The PC owns their
//! wiring and exposes display, audio and input handles to the embedding.
#![forbid(unsafe_code)]

#[cfg(feature = "audio")]
mod audio;
pub mod configuration;
mod error;
pub mod sound_card;
pub mod video_card;

pub use configuration::{Configuration, ParallelPort, SoundCard, VideoCard};
pub use error::InitializationError;
use relocade_bios::Keyboard;
use relocade_dma::Dma;
use relocade_frontend::{keyboard::KeyboardInput, mouse::MouseInput};
use relocade_machine::{real_mode_interrupt::Interrupts, Cpu, GuestClock, Runner};
use relocade_parallel::Port;
use relocade_pc_speaker::Speaker;
use relocade_pic::{Pic, KEYBOARD_IRQ, TIMER_IRQ};
use relocade_pit::Pit;
use relocade_vga::DisplaySnapshot;
use relocade_x86::CpuFeature;
use std::{cell::RefCell, rc::Rc};

/// Conventional RAM below the VGA aperture in the fixed PC memory map.
pub const CONVENTIONAL_MEMORY_SIZE: u32 = 640 * 1024;

/// One execution-owned PC with persistent CPU, memory, hardware and BIOS state.
pub struct Pc {
    runner: Runner,
    clock: GuestClock,
    dma: Rc<RefCell<Dma>>,
    video_card: video_card::VideoCard,
    speaker: Rc<RefCell<Speaker>>,
    sound_card: Option<Box<dyn sound_card::SoundCard>>,
    parallel_port: Option<Rc<RefCell<Port>>>,
    keyboard: Keyboard,
    inputs: relocade_ps2::Inputs,
    #[cfg(feature = "audio")]
    audio_attached: bool,
}

impl Pc {
    /// Initializes PC hardware and BIOS on the prepared runner. Create its
    /// machine and CPU using [`configuration::memory`] and [`configuration::cpu`].
    /// The CPU starts in real mode, ready for the caller to supply guest code.
    /// Display capture and sound hardware emulation require no platform output.
    pub async fn new(runner: Runner) -> Result<Self, InitializationError> {
        Self::with_configuration(runner, Configuration::default()).await
    }

    /// Initializes the selected hardware and BIOS on the prepared runner.
    /// The PC speaker remains available regardless of the installed sound card.
    /// Invalid resources return [`InitializationError::Validation`] before any
    /// hardware or BIOS state is installed.
    pub async fn with_configuration(
        runner: Runner,
        configuration: Configuration,
    ) -> Result<Self, InitializationError> {
        configuration
            .validate()
            .map_err(InitializationError::Validation)?;
        runner
            .initialize_guest_devices(
                configuration::DEFAULT_INSTRUCTIONS_PER_SECOND,
                async move |runner, clock| Self::assemble(runner, clock, configuration).await,
            )
            .await
    }

    async fn assemble(
        mut runner: Runner,
        clock: GuestClock,
        configuration: Configuration,
    ) -> Result<Self, InitializationError> {
        let dma = Dma::initialize(&runner).await;
        let video_card = configuration.video_card.initialize(&runner, &clock).await;
        let video_firmware = video_card.firmware();
        let pic = Pic::initialize(&runner).await;
        let pit = Pit::initialize(&runner, clock.clone(), &pic).await;
        let speaker = Speaker::initialize(&runner, pit).await;
        let sound_card = configuration
            .sound_card
            .initialize(&runner, &clock, &dma, &pic)
            .await;
        let parallel_port = match configuration.parallel_port {
            Some(port) => Some(Port::initialize(&runner, port.base, pic.irq(port.irq, false)).await),
            None => None,
        };
        let (inputs, controller) = relocade_ps2::connect(&runner, clock.clone(), &pic).await;
        let memory = runner.memory();
        let keyboard = Keyboard::new(memory.clone());
        let cpu = runner.cpu();
        {
            let mut context = cpu.context();
            let cpu_model = context.cpu.execution_target().cpu_model;
            relocade_bios::initialize(
                &mut context.memory,
                context.io,
                relocade_bios::Configuration {
                    conventional_memory_kib: (CONVENTIONAL_MEMORY_SIZE / 1024) as u16,
                    math_coprocessor: cpu_model.supports(CpuFeature::Fpu),
                    pointing_device: true,
                    parallel_ports: [configuration.parallel_port.map_or(0, |port| port.base), 0, 0],
                },
                &video_firmware,
            );
        }
        let mut interrupts = Interrupts::new(cpu);
        relocade_bios::register_interrupts(
            &mut interrupts,
            relocade_bios::Parallel::new(clock.clone().into(), parallel_port.iter().cloned()),
            keyboard.clone(),
            video_firmware,
            Some(relocade_bios::Mouse::new(clock.clone().into(), controller)),
        );
        interrupts.install().await?;
        pic.unmask_irq(TIMER_IRQ);
        pic.unmask_irq(KEYBOARD_IRQ);
        Ok(Self {
            runner,
            clock,
            dma,
            video_card,
            speaker,
            sound_card,
            parallel_port,
            keyboard,
            inputs,
            #[cfg(feature = "audio")]
            audio_attached: false,
        })
    }

    /// The emulation clock shared by the PC's devices and firmware consumers.
    pub fn clock(&self) -> &GuestClock {
        &self.clock
    }

    /// The execution session for connecting additional device activities.
    /// Configure devices while guest execution is stopped, inside Runner::run.
    pub fn runner(&self) -> &Runner {
        &self.runner
    }

    /// Accesses the CPU for guest setup, execution and additional service hooks.
    pub fn cpu(&mut self) -> &mut Cpu {
        self.runner.cpu()
    }

    /// Executes this PC's CPU with its scheduling and device activities.
    /// Await inside the execution session that supplied the runner; its outer
    /// [`Runner::run`] drives device activities and cancellation.
    pub async fn run(&mut self) -> relocade_x86::ExecutionExit {
        self.runner.run_cpu().await
    }

    /// The BIOS keyboard shared with INT 16h and console services.
    pub fn keyboard(&self) -> Keyboard {
        self.keyboard.clone()
    }

    /// Thread-safe physical input routed through the PS/2 controller and IRQ1.
    pub fn keyboard_input(&self) -> KeyboardInput {
        self.inputs.keyboard.clone()
    }

    /// Thread-safe relative mouse input routed through the PS/2 auxiliary port.
    /// INT 15h/C2h configures reporting and guest far callbacks through BIOS IRQ12.
    /// Raw drivers can instead own INT 74h and controller/PIC programming.
    /// An INT 33h driver is not installed.
    pub fn mouse_input(&self) -> MouseInput {
        self.inputs.mouse.clone()
    }

    /// Snapshot queue for one graphics consumer on another worker.
    pub fn frames(&self) -> async_channel::Receiver<DisplaySnapshot> {
        self.video_card.frames()
    }

    /// The installed card's frame delivery and native framebuffer access.
    pub fn video_card(&self) -> &video_card::VideoCard {
        &self.video_card
    }

    /// Retains the speaker hardware on its execution owner.
    pub fn speaker(&self) -> Rc<RefCell<Speaker>> {
        self.speaker.clone()
    }

    /// Retains the primary DMA controller for 8-bit peripheral transfers.
    pub fn dma(&self) -> Rc<RefCell<Dma>> {
        self.dma.clone()
    }

    /// Retains the configured parallel port for attaching a peripheral.
    /// Port presence does not imply that a peripheral is connected.
    pub fn parallel_port(&self) -> Option<Rc<RefCell<Port>>> {
        self.parallel_port.clone()
    }

    /// The common interface of the configured sound card, if present.
    pub fn sound_card(&self) -> Option<&dyn sound_card::SoundCard> {
        self.sound_card.as_deref()
    }
}
