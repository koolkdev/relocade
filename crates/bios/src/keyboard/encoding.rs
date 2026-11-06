//! IBM enhanced-keyboard buffer words and INT 16h compatibility translation.

pub(super) const EXTENDED_MARKER: u8 = 0xe0;
pub(super) const ENHANCED_ONLY_MARKER: u8 = 0xf0;
const LAST_LEGACY_SCAN_CODE: u8 = 0x84; // Ctrl+PageUp.

/// One BIOS character/scan-code pair, in the guest character encoding.
/// The guest ring retains E0/F0 markers; reads translate them for the chosen mode.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KeyStroke {
    pub character: u8,
    pub scan_code: u8,
}

/// The two INT 16h interfaces share one ring but expose different key encodings.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyMode {
    /// AH=00h/01h: translate compatible keys and discard enhanced-only entries.
    Legacy,
    /// AH=10h/11h: preserve extended identities and accept enhanced-only entries.
    Enhanced,
}

impl KeyMode {
    /// IBM KYBD's K10_S_XLAT and K10_E_XLAT, including scan-zero OEM characters.
    pub(super) fn translate(self, mut key: KeyStroke) -> Option<KeyStroke> {
        match self {
            Self::Enhanced => {
                if key.character == ENHANCED_ONLY_MARKER && key.scan_code != 0 {
                    key.character = 0;
                }
            }
            Self::Legacy => {
                if key.scan_code == EXTENDED_MARKER {
                    key.scan_code = if matches!(key.character, b'\r' | b'\n') {
                        0x1c
                    } else {
                        0x35
                    };
                } else if key.scan_code > LAST_LEGACY_SCAN_CODE
                    || (key.character == ENHANCED_ONLY_MARKER && key.scan_code != 0)
                {
                    return None;
                } else if key.character == EXTENDED_MARKER && key.scan_code != 0 {
                    key.character = 0;
                }
            }
        }
        Some(key)
    }
}
