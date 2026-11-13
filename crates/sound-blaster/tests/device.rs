#[path = "device/audio.rs"]
mod audio;
#[path = "device/high_speed.rs"]
mod high_speed;
#[path = "device/playback.rs"]
mod playback;
#[path = "device/ports.rs"]
mod ports;
#[path = "device/streaming.rs"]
mod streaming;

use relocade_clock_test_support::ManualClock;
use relocade_dma::Dma;
use relocade_machine::{Device, IoBus, MachineConfig, Memory};
use relocade_pic::Pic;
use relocade_sound_blaster::*;
use relocade_x86::{PhysicalMapping, PhysicalMemoryMap};
use std::{
    cell::RefCell,
    rc::Rc,
    task::{Context, Waker},
    time::Duration,
};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

struct Hardware {
    clock: ManualClock,
    io: IoBus,
    memory: Memory,
    card: Rc<RefCell<SoundBlaster>>,
}

impl Hardware {
    fn new() -> Self {
        let clock = ManualClock::new();
        let map =
            PhysicalMemoryMap::new([(0..=0xffff, PhysicalMapping::Ram { backing_offset: 0 })])
                .unwrap();
        let memory = Memory::new(MachineConfig::new(65536, map).unwrap()).unwrap();
        let io = IoBus::default();
        let dma = Rc::new(RefCell::new(Dma::default()));
        io.connect(dma.clone(), |io| Dma::configure_io(io, &dma));
        let pic = Pic::default();
        pic.unmask_irq(IRQ);
        let interrupts = Rc::new(RefCell::new(pic.clone()));
        io.connect(interrupts.clone(), |io| Pic::configure_io(io, &interrupts));
        let card = Rc::new(RefCell::new(SoundBlaster::new(
            clock.clock(),
            memory.clone(),
            &io,
            dma,
            &pic,
        )));
        io.connect(card.clone(), |io| SoundBlaster::configure_io(io, &card));
        Self {
            clock,
            io,
            memory,
            card,
        }
    }

    fn command(&self, bytes: &[u8]) {
        for &byte in bytes {
            self.io.write_port(WRITE_DATA_PORT, 1, byte.into());
        }
    }

    fn program_dma(&self, address: u16, count: u16) {
        use relocade_dma::*;
        for (port, value) in [
            (SINGLE_MASK_PORT, 5),
            (CLEAR_FLIP_FLOP_PORT, 0),
            (ADDRESS_PORTS[1], address as u8),
            (ADDRESS_PORTS[1], (address >> 8) as u8),
            (COUNT_PORTS[1], count as u8),
            (COUNT_PORTS[1], (count >> 8) as u8),
            (PAGE_PORTS[1], 0),
            (MODE_PORT, 0x49),
            (SINGLE_MASK_PORT, 1),
        ] {
            self.io.write_port(port, 1, value.into());
        }
    }

    fn dma_word(&self, port: u16) -> u16 {
        self.io.write_port(relocade_dma::CLEAR_FLIP_FLOP_PORT, 1, 0);
        self.io.read_port(port, 1) as u16 | ((self.io.read_port(port, 1) as u16) << 8)
    }

    fn acknowledge_irq(&self) {
        let mut cx = Context::from_waker(Waker::noop());
        assert_eq!(
            self.io.poll_interrupt(&mut cx),
            std::task::Poll::Ready(0x0f)
        );
        self.io.read_port(READ_STATUS_PORT, 1);
        relocade_pic::end_of_interrupt(&self.io, IRQ);
    }
}
