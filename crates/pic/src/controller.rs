use std::task::Waker;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Initialization {
    Ready,
    Vector,
    Cascade,
    Mode,
}

pub(super) struct Controller {
    pub vector_base: u8,
    pub mask: u8,
    requests: u8,
    in_service: u8,
    levels: u8,
    first: u8,
    level_triggered: bool,
    auto_eoi: bool,
    rotate_auto: bool,
    special_mask: bool,
    read_service: bool,
    poll: bool,
    initialization: Initialization,
    single: bool,
    needs_mode: bool,
    pub waker: Option<Waker>,
}

impl Controller {
    pub fn new(vector_base: u8) -> Self {
        Self {
            vector_base,
            mask: 0xff,
            requests: 0,
            in_service: 0,
            levels: 0,
            first: 0,
            level_triggered: false,
            auto_eoi: false,
            rotate_auto: false,
            special_mask: false,
            read_service: false,
            poll: false,
            initialization: Initialization::Ready,
            single: true,
            needs_mode: false,
            waker: None,
        }
    }

    fn priority(&self, bits: u8) -> Option<u8> {
        (0..8)
            .map(|offset| (self.first + offset) & 7)
            .find(|irq| bits & (1 << irq) != 0)
    }

    fn blocking_service(&self) -> u8 {
        self.in_service & if self.special_mask { !self.mask } else { 0xff }
    }

    pub fn next_irq(&self) -> Option<u8> {
        if self.initialization != Initialization::Ready {
            return None;
        }
        let pending = self.priority(self.requests & !self.mask)?;
        if let Some(active) = self.priority(self.blocking_service()) {
            if pending.wrapping_sub(self.first) & 7 >= active.wrapping_sub(self.first) & 7 {
                return None;
            }
        }
        Some(pending)
    }

    pub fn acknowledge(&mut self) -> Option<u8> {
        let irq = self.next_irq()?;
        self.requests &= !(1 << irq);
        if self.auto_eoi {
            if self.rotate_auto {
                self.first = (irq + 1) & 7;
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
        if high {
            if self.levels & bit == 0 {
                self.requests |= bit;
            }
            self.levels |= bit;
        } else {
            self.levels &= !bit;
        }
        self.refresh_levels();
    }

    pub fn read_command(&mut self) -> u8 {
        if std::mem::take(&mut self.poll) {
            self.acknowledge().map_or(0, |irq| 0x80 | irq)
        } else if self.read_service {
            self.in_service
        } else {
            self.requests
        }
    }

    pub fn command(&mut self, value: u8) {
        if value & 0x10 != 0 {
            let waker = self.waker.take();
            let levels = self.levels;
            *self = Self::new(0);
            self.waker = waker;
            self.levels = levels;
            self.mask = 0;
            self.level_triggered = value & 8 != 0;
            self.single = value & 2 != 0;
            self.needs_mode = value & 1 != 0;
            self.initialization = Initialization::Vector;
        } else if value & 8 != 0 {
            if value & 2 != 0 {
                self.read_service = value & 1 != 0;
            }
            if value & 0x40 != 0 {
                self.special_mask = value & 0x20 != 0;
            }
            self.poll = value & 4 != 0;
        } else {
            let operation = value >> 5;
            let irq = if value & 0x40 != 0 {
                Some(value & 7)
            } else {
                self.priority(self.blocking_service())
            };
            match operation {
                0 => self.rotate_auto = false,
                4 => self.rotate_auto = true,
                6 => self.first = ((value & 7) + 1) & 7,
                1 | 3 | 5 | 7 => {
                    if let Some(irq) = irq {
                        self.in_service &= !(1 << irq);
                        if value & 0x80 != 0 {
                            self.first = (irq + 1) & 7;
                        }
                    }
                }
                _ => {}
            }
        }
    }

    pub fn data(&mut self, value: u8) {
        use Initialization::*;
        self.initialization = match self.initialization {
            Ready => {
                self.mask = value;
                Ready
            }
            Vector => {
                self.vector_base = value & 0xf8;
                if !self.single {
                    Cascade
                } else if self.needs_mode {
                    Mode
                } else {
                    Ready
                }
            }
            // The cascade wiring is accepted for standard PC initialization;
            // only the master's eight inputs are connected by this device.
            Cascade => {
                if self.needs_mode {
                    Mode
                } else {
                    Ready
                }
            }
            Mode => {
                self.auto_eoi = value & 2 != 0;
                Ready
            }
        };
        self.refresh_levels();
    }
}
