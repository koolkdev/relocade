//! Incremental escape parsing with bounded numeric parameters.

#[derive(Default)]
pub(super) enum Parser {
    #[default]
    Text,
    Escape,
    Csi(Parameters),
}

#[derive(Default)]
pub(super) struct Parameters {
    values: [u16; 16],
    index: usize,
    invalid: bool,
}

pub(super) enum Command {
    Character(u8),
    Control(u8, Parameters),
}

impl Parameters {
    pub fn values(&self) -> &[u16] {
        &self.values[..=self.index]
    }

    pub fn get(&self, index: usize, default: u16) -> u16 {
        self.values()
            .get(index)
            .copied()
            .filter(|&value| value != 0)
            .unwrap_or(default)
    }
}

impl Parser {
    pub fn push(&mut self, byte: u8) -> Option<Command> {
        if byte == 0x1b {
            *self = Self::Escape;
            return None;
        }
        match self {
            Self::Text => Some(Command::Character(byte)),
            Self::Escape => {
                *self = if byte == b'[' {
                    Self::Csi(Parameters::default())
                } else {
                    Self::Text
                };
                None
            }
            Self::Csi(parameters) => {
                match byte {
                    b'0'..=b'9' => {
                        let value = parameters.values[parameters.index];
                        if let Some(value) = value
                            .checked_mul(10)
                            .and_then(|value| value.checked_add(u16::from(byte - b'0')))
                        {
                            parameters.values[parameters.index] = value;
                        } else {
                            parameters.invalid = true;
                        }
                    }
                    b';' if parameters.index + 1 < parameters.values.len() => {
                        parameters.index += 1;
                    }
                    0x40..=0x7e => {
                        let Self::Csi(parameters) = std::mem::take(self) else {
                            unreachable!()
                        };
                        return (!parameters.invalid).then_some(Command::Control(byte, parameters));
                    }
                    0x18 | 0x1a => *self = Self::Text, // Cancel an unfinished sequence.
                    _ => parameters.invalid = true,
                }
                None
            }
        }
    }
}
