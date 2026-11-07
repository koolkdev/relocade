pub(super) const IRQ_COUNT: u8 = 8;
const IRQ_NUMBER_MASK: u8 = IRQ_COUNT - 1;

// Initialization Command Words (ICW1 and ICW4).
const ICW1_INITIALIZE: u8 = 1 << 4;
const ICW1_LEVEL_TRIGGERED: u8 = 1 << 3;
const ICW1_SINGLE_CONTROLLER: u8 = 1 << 1;
const ICW1_EXPECT_ICW4: u8 = 1;
const ICW4_AUTO_EOI: u8 = 1 << 1;

// Operation Command Word 2: end of interrupt and priority rotation.
pub(super) const OCW2_EOI: u8 = 1 << 5;
const OCW2_SPECIFIC_IRQ: u8 = 1 << 6;
const OCW2_ROTATE: u8 = 1 << 7;

// Operation Command Word 3: register reads, polling and special mask mode.
const OCW3_SELECT: u8 = 1 << 3;
const OCW3_SELECT_READ_REGISTER: u8 = 1 << 1;
const OCW3_READ_IN_SERVICE: u8 = 1;
const OCW3_POLL: u8 = 1 << 2;
const OCW3_CHANGE_SPECIAL_MASK: u8 = 1 << 6;
const OCW3_SPECIAL_MASK: u8 = 1 << 5;
const POLL_HAS_INTERRUPT: u8 = 1 << 7;

#[derive(Clone, Copy)]
enum Initialization {
    Ready,
    VectorBase {
        needs_cascade: bool,
        needs_icw4: bool,
    },
    Cascade {
        needs_icw4: bool,
    },
    Mode,
}

pub(super) struct Controller {
    pub vector_base: u8,
    // Interrupt Mask, Interrupt Request and In-Service registers (IMR/IRR/ISR).
    pub mask: u8,
    requests: u8,
    in_service: u8,
    levels: u8,
    highest_priority_irq: u8,
    level_triggered: bool,
    auto_eoi: bool,
    rotate_on_auto_eoi: bool,
    special_mask: bool,
    read_in_service: bool,
    pub poll_pending: bool,
    pub cascade: u8,
    pub single: bool,
    initialization: Initialization,
}

impl Controller {
    pub fn new(vector_base: u8, cascade: u8) -> Self {
        Self {
            vector_base,
            mask: u8::MAX,
            requests: 0,
            in_service: 0,
            levels: 0,
            highest_priority_irq: 0,
            level_triggered: false,
            auto_eoi: false,
            rotate_on_auto_eoi: false,
            special_mask: false,
            read_in_service: false,
            poll_pending: false,
            initialization: Initialization::Ready,
            cascade,
            single: false,
        }
    }

    fn highest_priority(&self, bits: u8) -> Option<u8> {
        (0..IRQ_COUNT)
            .map(|offset| (self.highest_priority_irq + offset) % IRQ_COUNT)
            .find(|irq| bits & (1 << irq) != 0)
    }

    // Rank zero is highest; rotation wraps around the eight interrupt inputs.
    fn priority_rank(&self, irq: u8) -> u8 {
        irq.wrapping_sub(self.highest_priority_irq) & IRQ_NUMBER_MASK
    }

    fn set_lowest_priority(&mut self, irq: u8) {
        self.highest_priority_irq = (irq + 1) % IRQ_COUNT;
    }

    fn blocking_service(&self) -> u8 {
        if self.special_mask {
            self.in_service & !self.mask
        } else {
            self.in_service
        }
    }

    pub fn next_irq(&self) -> Option<u8> {
        if !matches!(self.initialization, Initialization::Ready) {
            return None;
        }
        let pending = self.highest_priority(self.requests & !self.mask)?;
        if let Some(active) = self.highest_priority(self.blocking_service()) {
            if self.priority_rank(pending) >= self.priority_rank(active) {
                return None;
            }
        }
        Some(pending)
    }

    pub fn acknowledge(&mut self) -> Option<u8> {
        let irq = self.next_irq()?;
        self.requests &= !(1 << irq);
        if self.auto_eoi {
            if self.rotate_on_auto_eoi {
                self.set_lowest_priority(irq);
            }
        } else {
            self.in_service |= 1 << irq;
        }
        self.refresh_levels();
        Some(irq)
    }

    fn refresh_levels(&mut self) {
        if self.level_triggered {
            self.requests = self.levels;
        }
    }

    pub fn set_level(&mut self, irq: u8, high: bool) {
        let bit = 1 << irq;
        if high && self.levels & bit == 0 {
            self.requests |= bit;
        }
        self.record_level(irq, high);
    }

    pub fn record_level(&mut self, irq: u8, high: bool) {
        let bit = 1 << irq;
        if high {
            self.levels |= bit;
        } else {
            self.levels &= !bit;
        }
        self.refresh_levels();
    }

    pub fn read_command(&mut self) -> u8 {
        if std::mem::take(&mut self.poll_pending) {
            self.acknowledge().map_or(0, |irq| POLL_HAS_INTERRUPT | irq)
        } else if self.read_in_service {
            self.in_service
        } else {
            self.requests
        }
    }

    pub fn command(&mut self, value: u8) {
        if value & ICW1_INITIALIZE != 0 {
            self.initialize(value);
        } else if value & OCW3_SELECT != 0 {
            self.write_ocw3(value);
        } else {
            self.write_ocw2(value);
        }
    }

    fn initialize(&mut self, value: u8) {
        let levels = self.levels;
        *self = Self::new(0, 7);
        self.single = value & ICW1_SINGLE_CONTROLLER != 0;
        self.levels = levels;
        self.mask = 0;
        self.level_triggered = value & ICW1_LEVEL_TRIGGERED != 0;
        self.initialization = Initialization::VectorBase {
            needs_cascade: value & ICW1_SINGLE_CONTROLLER == 0,
            needs_icw4: value & ICW1_EXPECT_ICW4 != 0,
        };
    }

    fn write_ocw2(&mut self, value: u8) {
        let specific = value & OCW2_SPECIFIC_IRQ != 0;
        let rotate = value & OCW2_ROTATE != 0;
        if value & OCW2_EOI != 0 {
            let irq = if specific {
                Some(value & IRQ_NUMBER_MASK)
            } else {
                self.highest_priority(self.blocking_service())
            };
            if let Some(irq) = irq {
                self.in_service &= !(1 << irq);
                if rotate {
                    self.set_lowest_priority(irq);
                }
            }
        } else if !specific {
            self.rotate_on_auto_eoi = rotate;
        } else if rotate {
            self.set_lowest_priority(value & IRQ_NUMBER_MASK);
        }
    }

    fn write_ocw3(&mut self, value: u8) {
        if value & OCW3_SELECT_READ_REGISTER != 0 {
            self.read_in_service = value & OCW3_READ_IN_SERVICE != 0;
        }
        if value & OCW3_CHANGE_SPECIAL_MASK != 0 {
            self.special_mask = value & OCW3_SPECIAL_MASK != 0;
        }
        self.poll_pending = value & OCW3_POLL != 0;
    }

    pub fn data(&mut self, value: u8) {
        use Initialization::*;
        self.initialization = match self.initialization {
            Ready => {
                self.mask = value;
                Ready
            }
            VectorBase {
                needs_cascade,
                needs_icw4,
            } => {
                // ICW2 supplies the upper vector bits; the IRQ supplies the low three.
                self.vector_base = value & !IRQ_NUMBER_MASK;
                if needs_cascade {
                    Cascade { needs_icw4 }
                } else if needs_icw4 {
                    Mode
                } else {
                    Ready
                }
            }
            Cascade { needs_icw4 } => {
                self.cascade = value;
                if needs_icw4 {
                    Mode
                } else {
                    Ready
                }
            }
            Mode => {
                self.auto_eoi = value & ICW4_AUTO_EOI != 0;
                Ready
            }
        };
        self.refresh_levels();
    }
}
