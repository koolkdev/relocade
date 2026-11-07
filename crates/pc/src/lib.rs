//! A BIOS-initialized PC, with hardware and firmware independent of DOS.
//!
//! [`Pc::new`] assembles the CPU, memory, PIC, PIT, PS/2 keyboard/mouse, VGA and speaker
//! on an existing execution runner. It initializes BIOS data and interrupt
//! services. The caller supplies guest code and its entry state; this does not
//! execute a reset ROM, POST or a disk bootstrap.
//!
//! Devices retain their own emulation and async activities. The PC owns their
//! wiring and exposes display, audio and input handles to the embedding.
#![forbid(unsafe_code)]

mod configuration;

use relocade_bios::Keyboard;
use relocade_frontend::{keyboard::KeyboardInput, mouse::MouseInput};
use relocade_machine::{
    real_mode_interrupt::Interrupts, Clock, Cpu, IoBus, Memory, Runner, TimerHandle,
};
use relocade_pc_speaker::Speaker;
use relocade_pic::{Pic, KEYBOARD_IRQ, TIMER_IRQ};
use relocade_pit::Pit;
use relocade_vga::{FrameSource, Vga};
use relocade_x86::CpuFeature;
use std::{cell::RefCell, rc::Rc};

/// Conventional RAM below the VGA aperture in the fixed PC memory map.
pub const CONVENTIONAL_MEMORY_SIZE: u32 = 640 * 1024;

/// One execution-owned PC with persistent CPU, memory, hardware and BIOS state.
pub struct Pc<C = TimerHandle> {
    cpu: Cpu,
    vga: Rc<RefCell<Vga<C>>>,
    speaker: Rc<RefCell<Speaker<C>>>,
    keyboard: Keyboard,
    inputs: relocade_ps2::Inputs,
}

impl<C: Clock> Pc<C> {
    /// Initializes the fixed PC hardware and BIOS on the runner's clock.
    /// The CPU starts in real mode, ready for the caller to supply guest code.
    /// Display capture and speaker emulation require no platform output.
    pub async fn new(runner: &Runner<C>) -> relocade_machine::Result<Self>
    where
        C: Clone + 'static,
    {
        let (vga, vga_device) = Vga::initialize(runner).await;
        let (pic, pic_device) = Pic::initialize(runner).await;
        let (pit, pit_device) = Pit::initialize(runner, &pic).await;
        let (speaker, speaker_device) = Speaker::initialize(runner, pit).await;
        let (inputs, ps2_device) = relocade_ps2::connect(runner, &pic).await;
        let io = IoBus::new([
            pic_device,
            pit_device,
            speaker_device,
            vga_device,
            ps2_device,
        ]);
        let memory = Memory::new(configuration::memory())?;
        let keyboard = Keyboard::new(memory.clone());
        let mut cpu = runner.create_cpu(memory, configuration::cpu(), io).await?;
        {
            let mut context = cpu.context();
            let cpu_model = context.cpu.execution_target().cpu_model;
            relocade_bios::initialize(
                &mut context.memory,
                context.io,
                relocade_bios::Configuration {
                    conventional_memory_kib: (CONVENTIONAL_MEMORY_SIZE / 1024) as u16,
                    math_coprocessor: cpu_model.supports(CpuFeature::Fpu),
                },
            );
        }
        let mut interrupts = Interrupts::new(&mut cpu);
        relocade_bios::register_interrupts(&mut interrupts, keyboard.clone(), vga.clone());
        interrupts.install().await?;
        pic.unmask_irq(TIMER_IRQ);
        pic.unmask_irq(KEYBOARD_IRQ);
        Ok(Self {
            cpu,
            vga,
            speaker,
            keyboard,
            inputs,
        })
    }

    /// Accesses the CPU for guest setup, execution and additional service hooks.
    pub fn cpu(&mut self) -> &mut Cpu {
        &mut self.cpu
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
    /// Guest software enables reporting, installs INT 74h and unmasks IRQ2/IRQ12.
    /// No BIOS mouse service or INT 33h driver is installed yet.
    pub fn mouse_input(&self) -> MouseInput {
        self.inputs.mouse.clone()
    }

    /// Immutable display captures for rendering on another worker.
    pub fn frame_source(&self) -> FrameSource {
        self.vga.borrow().frame_source()
    }

    /// Retains the VGA device on its execution owner. End borrows before polling.
    pub fn vga(&self) -> Rc<RefCell<Vga<C>>> {
        self.vga.clone()
    }

    /// Retains the speaker for attaching audio output on its execution owner.
    pub fn speaker(&self) -> Rc<RefCell<Speaker<C>>> {
        self.speaker.clone()
    }
}
