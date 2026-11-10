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
use relocade_machine::{real_mode_interrupt::Interrupts, Clock, Cpu, IoBus, Memory, Runner};
use relocade_pc_speaker::Speaker;
use relocade_pic::{Pic, KEYBOARD_IRQ, TIMER_IRQ};
use relocade_pit::Pit;
use relocade_vga::{DisplaySnapshot, Vga};
use relocade_x86::CpuFeature;
use std::{cell::RefCell, rc::Rc};

/// Conventional RAM below the VGA aperture in the fixed PC memory map.
pub const CONVENTIONAL_MEMORY_SIZE: u32 = 640 * 1024;

/// One execution-owned PC with persistent CPU, memory, hardware and BIOS state.
pub struct Pc {
    runner: Runner,
    vga: Rc<RefCell<Vga>>,
    speaker: Rc<RefCell<Speaker>>,
    keyboard: Keyboard,
    inputs: relocade_ps2::Inputs,
}

impl Pc {
    /// Initializes the fixed PC hardware and BIOS on the runner's clock.
    /// The CPU starts in real mode, ready for the caller to supply guest code.
    /// Display capture and speaker emulation require no platform output.
    pub async fn new(mut runner: Runner) -> relocade_machine::Result<Self> {
        let (vga, vga_device) = Vga::initialize(&runner).await;
        let (pic, pic_device) = Pic::initialize(&runner).await;
        let (pit, pit_device) = Pit::initialize(&runner, &pic).await;
        let (speaker, speaker_device) = Speaker::initialize(&runner, pit).await;
        let (inputs, controller, ps2_device) = relocade_ps2::connect(&runner, &pic).await;
        let io = IoBus::new([
            pic_device,
            pit_device,
            speaker_device,
            vga_device,
            ps2_device,
        ]);
        let memory = Memory::new(configuration::memory())?;
        let keyboard = Keyboard::new(memory.clone());
        let clock = runner.clock().clone();
        let cpu = runner.create_cpu(memory, configuration::cpu(), io).await?;
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
                },
            );
        }
        let mut interrupts = Interrupts::new(cpu);
        relocade_bios::register_interrupts(
            &mut interrupts,
            keyboard.clone(),
            vga.clone(),
            Some(relocade_bios::Mouse::new(clock, controller)),
        );
        interrupts.install().await?;
        pic.unmask_irq(TIMER_IRQ);
        pic.unmask_irq(KEYBOARD_IRQ);
        Ok(Self {
            runner,
            vga,
            speaker,
            keyboard,
            inputs,
        })
    }

    /// The emulation clock shared by the PC's devices and firmware consumers.
    pub fn clock(&self) -> &Clock {
        self.runner.clock()
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
        self.vga.borrow().frames()
    }

    /// Retains the VGA device on its execution owner. End borrows before polling.
    pub fn vga(&self) -> Rc<RefCell<Vga>> {
        self.vga.clone()
    }

    /// Retains the speaker for attaching audio output on its execution owner.
    pub fn speaker(&self) -> Rc<RefCell<Speaker>> {
        self.speaker.clone()
    }
}
