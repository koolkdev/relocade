use crate::DMA_CHANNEL;
use relocade_dma::Dma;
use relocade_machine::{Clock, IoContext, Memory, Timestamp};
use relocade_pic::IrqLine;
use std::{cell::RefCell, rc::Rc, time::Duration};

struct Transfer {
    /// Last elapsed sample boundary, retaining the clock's fractional phase.
    at: Timestamp,
    remaining: u32,
}

pub(super) struct Playback {
    clock: Clock,
    memory: Memory,
    pub(super) dma: Rc<RefCell<Dma>>,
    irq: IrqLine,
    transfer: Option<Transfer>,
    time_constant: u8,
}

impl Playback {
    pub fn new(clock: Clock, memory: Memory, dma: Rc<RefCell<Dma>>, irq: IrqLine) -> Self {
        Self {
            clock,
            memory,
            dma,
            irq,
            transfer: None,
            time_constant: 0,
        }
    }

    pub fn reset(&mut self) {
        self.transfer = None;
        self.time_constant = 0;
        self.acknowledge();
    }

    pub fn acknowledge(&self) {
        self.irq.set_level(false);
    }

    pub fn set_time_constant(&mut self, value: u8) {
        self.time_constant = value;
        if let Some(transfer) = &mut self.transfer {
            transfer.at = self.clock.now();
        }
    }

    pub fn start(&mut self, count: u16) {
        self.transfer = Some(Transfer {
            at: self.clock.now(),
            remaining: u32::from(count) + 1,
        });
    }

    fn period(&self) -> Duration {
        Duration::from_micros(256 - u64::from(self.time_constant))
    }

    pub fn is_due(&self) -> bool {
        self.transfer.as_ref().is_some_and(|transfer| {
            self.clock.duration_since(self.clock.now(), transfer.at) >= self.period()
        })
    }

    pub fn advance(&mut self, context: IoContext<'_>) {
        let period = self.period();
        let Some(transfer) = &mut self.transfer else {
            return;
        };
        let elapsed = self.clock.duration_since(self.clock.now(), transfer.at);
        let due = elapsed.as_nanos() / period.as_nanos();
        if due == 0 {
            return;
        }
        let mut samples = vec![0; due.min(u128::from(transfer.remaining)) as usize];
        let memory = self.memory.view().with_io_context(context);
        let consumed = self
            .dma
            .borrow_mut()
            .read(DMA_CHANNEL, &memory, &mut samples);
        // Masked or exhausted DMA misses these slots; a later unmask starts now.
        let remainder = Duration::from_nanos((elapsed.as_nanos() % period.as_nanos()) as u64);
        transfer.at = self.clock.add(transfer.at, elapsed - remainder);
        transfer.remaining -= consumed as u32;
        if transfer.remaining == 0 {
            self.transfer = None;
            self.irq.set_level(true);
        }
    }

    pub fn deadline(&self) -> Option<Timestamp> {
        self.transfer.as_ref().map(|transfer| {
            // Batch elapsed samples at about 1 ms, or sooner for completion.
            let batch = (1_000 / self.period().as_micros() as u32).max(1);
            self.clock
                .add(transfer.at, self.period() * transfer.remaining.min(batch))
        })
    }
}
