//! A descriptor's encoded limit and granularity remain one bounded value.

/// An inclusive 20-bit descriptor limit with byte or 4 KiB page granularity.
/// Loaded segment caches store its expanded [`Self::effective`] byte limit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SegmentLimit {
    encoded: u32,
    page_granular: bool,
}

impl SegmentLimit {
    /// Constructs a byte-granular limit. Values above 0xfffff are unrepresentable.
    pub const fn bytes(limit: u32) -> Option<Self> {
        if limit > 0xfffff {
            return None;
        }
        Some(Self {
            encoded: limit,
            page_granular: false,
        })
    }

    /// Constructs a page-granular limit from an inclusive page index (0..=0xfffff).
    /// For example, page index zero includes bytes 0..=0xfff.
    pub const fn pages(limit: u32) -> Option<Self> {
        if limit > 0xfffff {
            return None;
        }
        Some(Self {
            encoded: limit,
            page_granular: true,
        })
    }

    /// Converts an effective byte limit without rounding, preferring G=0 when
    /// possible. Larger limits must end at a page boundary. Use [`Self::pages`]
    /// to request G=1 explicitly for a small limit.
    pub const fn from_effective(limit: u32) -> Option<Self> {
        if limit <= 0xfffff {
            Self::bytes(limit)
        } else if limit & 0xfff == 0xfff {
            Self::pages(limit >> 12)
        } else {
            None
        }
    }

    /// Returns the inclusive effective byte limit used by LSL and segment caches.
    pub const fn effective(self) -> u32 {
        if self.page_granular {
            (self.encoded << 12) | 0xfff
        } else {
            self.encoded
        }
    }

    /// Returns the descriptor's granularity bit (G).
    pub const fn is_page_granular(self) -> bool {
        self.page_granular
    }
}

#[cfg(test)]
mod tests;
