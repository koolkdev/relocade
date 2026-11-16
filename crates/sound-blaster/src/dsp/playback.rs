use super::dac::Dac;
use crate::DMA_CHANNEL;
use relocade_dma::Dma;
use relocade_machine::{Clock, IoContext, Memory, Timestamp};
use relocade_pic::IrqLine;
use std::{cell::RefCell, rc::Rc, time::Duration};

#[derive(Clone, Copy)]
enum Mode {
    Single,
    AutoInitialize,
    ExitAutoInitialize,
    SingleAfterBlock(u32),
    HighSpeedSingle,
    HighSpeedAutoInitialize,
    Silence,
}

impl Mode {
    fn auto_initialize(self) -> bool {
        matches!(
            self,
            Self::AutoInitialize
                | Self::ExitAutoInitialize
                | Self::SingleAfterBlock(_)
                | Self::HighSpeedAutoInitialize
        )
    }
}

struct Transfer {
    /// Native anchor and elapsed slots on the DSP sample grid.
    started_at: Timestamp,
    samples: u64,
    remaining: u32,
    mode: Mode,
    paused_at: Option<Timestamp>,
}

pub(super) struct Playback {
    clock: Clock,
    memory: Memory,
    pub(super) dma: Rc<RefCell<Dma>>,
    irq: IrqLine,
    transfer: Option<Transfer>,
    time_constant: u8,
    block_size: u32,
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
            block_size: 65_536,
        }
    }

    pub fn reset(&mut self) {
        self.stop();
        self.time_constant = 0;
        self.block_size = 65_536;
    }

    /// Aborts the transfer while retaining DSP rate and block-size programming.
    pub fn stop(&mut self) {
        self.transfer = None;
        self.acknowledge();
    }

    pub fn acknowledge(&self) {
        self.irq.set_level(false);
    }

    pub fn set_time_constant(&mut self, value: u8) {
        self.time_constant = value;
        if let Some(transfer) = &mut self.transfer {
            transfer.started_at = transfer.paused_at.unwrap_or_else(|| self.clock.now());
            transfer.samples = 0;
        }
    }

    pub fn set_block_size(&mut self, count: u16) {
        self.block_size = u32::from(count) + 1;
    }

    pub fn start(&mut self, count: u16) {
        let count = u32::from(count) + 1;
        if let Some(transfer) = &mut self.transfer {
            if transfer.mode.auto_initialize() {
                transfer.mode = Mode::SingleAfterBlock(count);
                return;
            }
        }
        self.begin(count, Mode::Single);
    }

    pub fn start_auto_initialize(&mut self) {
        self.begin(self.block_size, Mode::AutoInitialize);
    }

    pub fn start_high_speed(&mut self, auto_initialize: bool) {
        let mode = if auto_initialize {
            Mode::HighSpeedAutoInitialize
        } else {
            Mode::HighSpeedSingle
        };
        self.begin(self.block_size, mode);
    }

    pub fn is_high_speed(&self) -> bool {
        self.transfer.as_ref().is_some_and(|transfer| {
            matches!(
                transfer.mode,
                Mode::HighSpeedSingle | Mode::HighSpeedAutoInitialize
            )
        })
    }

    pub fn silence(&mut self, count: u16, dac: &mut Dac) {
        self.begin(u32::from(count) + 1, Mode::Silence);
        dac.write(self.clock.now(), 0x80);
    }

    fn begin(&mut self, count: u32, mode: Mode) {
        self.transfer = Some(Transfer {
            started_at: self.clock.now(),
            samples: 0,
            remaining: count,
            mode,
            paused_at: None,
        });
    }

    pub fn pause(&mut self) {
        if let Some(transfer) = &mut self.transfer {
            if !matches!(transfer.mode, Mode::Silence) {
                transfer.paused_at.get_or_insert_with(|| self.clock.now());
            }
        }
    }

    pub fn resume(&mut self) {
        if let Some(transfer) = &mut self.transfer {
            if let Some(paused_at) = transfer.paused_at.take() {
                let paused = self.clock.now().ticks().wrapping_sub(paused_at.ticks());
                transfer.started_at =
                    Timestamp::from_ticks(transfer.started_at.ticks().wrapping_add(paused));
            }
        }
    }

    pub fn exit_auto_initialize(&mut self) {
        if let Some(transfer) = &mut self.transfer {
            if transfer.mode.auto_initialize() {
                transfer.mode = Mode::ExitAutoInitialize;
            }
        }
    }

    fn period(&self) -> Duration {
        Duration::from_micros(256 - u64::from(self.time_constant))
    }

    pub fn is_due(&self) -> bool {
        self.transfer.as_ref().is_some_and(|transfer| {
            transfer.paused_at.is_none()
                && self.clock.now()
                    >= self.clock.deadline_after(
                        transfer.started_at,
                        sample_delay(self.period(), transfer.samples + 1),
                    )
        })
    }

    pub fn advance(&mut self, context: IoContext<'_>, dac: &mut Dac) {
        let period = self.period();
        let Some(transfer) = &mut self.transfer else {
            return;
        };
        if transfer.paused_at.is_some() {
            return;
        }
        let elapsed = self
            .clock
            .duration_between(transfer.started_at, self.clock.now());
        let completed =
            u64::try_from(elapsed.as_nanos() / period.as_nanos()).expect("DSP sample count fits");
        let mut due = completed - transfer.samples;
        if due == 0 {
            return;
        }
        let memory = self.memory.view().with_io_context(context);
        while due != 0 {
            let requested = due.min(u64::from(transfer.remaining)) as usize;
            let consumed = if matches!(transfer.mode, Mode::Silence) {
                requested
            } else {
                let mut samples = vec![0; requested];
                let consumed = self
                    .dma
                    .borrow_mut()
                    .read(DMA_CHANNEL, &memory, &mut samples);
                samples.truncate(consumed);
                dac.write_samples(transfer.started_at, transfer.samples + 1, period, samples);
                consumed
            };
            transfer.samples += consumed as u64;
            transfer.remaining -= consumed as u32;
            due -= consumed as u64;
            if transfer.remaining == 0 {
                // Multiple completed blocks share the DSP's latched IRQ line.
                self.irq.set_level(true);
                match transfer.mode {
                    Mode::Single
                    | Mode::ExitAutoInitialize
                    | Mode::HighSpeedSingle
                    | Mode::Silence => {
                        self.transfer = None;
                        return;
                    }
                    Mode::AutoInitialize | Mode::HighSpeedAutoInitialize => {
                        transfer.remaining = self.block_size
                    }
                    Mode::SingleAfterBlock(count) => {
                        transfer.remaining = count;
                        transfer.mode = Mode::Single;
                    }
                }
            }
            if consumed < requested {
                // Missed DMA slots are discarded before a later unmask/refill.
                transfer.samples = completed;
                return;
            }
        }
    }

    pub fn deadline(&self) -> Option<Timestamp> {
        self.transfer
            .as_ref()
            .filter(|t| t.paused_at.is_none())
            .map(|transfer| {
                if matches!(transfer.mode, Mode::Silence) {
                    return self.clock.deadline_after(
                        transfer.started_at,
                        sample_delay(
                            self.period(),
                            transfer.samples + u64::from(transfer.remaining),
                        ),
                    );
                }
                // Batch elapsed samples at about 1 ms, or sooner for completion.
                let batch = (1_000 / self.period().as_micros() as u32).max(1);
                self.clock.deadline_after(
                    transfer.started_at,
                    sample_delay(
                        self.period(),
                        transfer.samples + u64::from(transfer.remaining.min(batch)),
                    ),
                )
            })
    }
}

/// Absolute offset on the DSP sample grid; no per-period deadline rounding.
pub(super) fn sample_delay(period: Duration, samples: u64) -> Duration {
    let nanos = period.as_nanos() * u128::from(samples);
    Duration::new(
        u64::try_from(nanos / 1_000_000_000).expect("DSP duration fits"),
        (nanos % 1_000_000_000) as u32,
    )
}
