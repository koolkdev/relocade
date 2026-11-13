//! ANSI rendition state mapped to IBM text attributes.

pub(super) struct Attributes {
    foreground: u8,
    background: u8,
    intense: bool,
    blink: bool,
    reverse: bool,
    concealed: bool,
}

impl Default for Attributes {
    fn default() -> Self {
        Self {
            foreground: 7,
            background: 0,
            intense: false,
            blink: false,
            reverse: false,
            concealed: false,
        }
    }
}

impl Attributes {
    pub fn set(&mut self, codes: &[u16]) {
        // ANSI orders colors as RGB bits; IBM text attributes use BGR bits.
        const COLORS: [u8; 8] = [0, 4, 2, 6, 1, 5, 3, 7];
        for &code in codes {
            match code {
                0 => *self = Self::default(),
                1 => self.intense = true,
                5 => self.blink = true,
                7 => self.reverse = true,
                8 => self.concealed = true,
                22 => self.intense = false,
                25 => self.blink = false,
                27 => self.reverse = false,
                28 => self.concealed = false,
                30..=37 => self.foreground = COLORS[usize::from(code - 30)],
                39 => self.foreground = 7,
                40..=47 => self.background = COLORS[usize::from(code - 40)],
                49 => self.background = 0,
                _ => {}
            }
        }
    }

    pub fn byte(&self) -> u8 {
        let (mut foreground, background) = if self.reverse {
            (self.background, self.foreground)
        } else {
            (self.foreground, self.background)
        };
        if self.concealed {
            foreground = background;
        }
        foreground
            | (background << 4)
            | (u8::from(self.intense && !self.concealed) << 3)
            | (u8::from(self.blink) << 7)
    }
}
