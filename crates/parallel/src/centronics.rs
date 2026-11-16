//! Centronics signals and direct byte output feeding one asynchronous reader.

mod connection;
mod stream;
pub use connection::Centronics;
pub use stream::{Input, Output};

/// Device status independent of connector polarity and stream capacity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Status {
    /// Device readiness in addition to the connection's one-byte capacity.
    pub ready: bool,
    pub selected: bool,
    pub paper_empty: bool,
    pub error: bool,
}

impl Default for Status {
    fn default() -> Self {
        Self {
            ready: true,
            selected: true,
            paper_empty: false,
            error: false,
        }
    }
}

/// Sideband requests; the connection does not transform the byte stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Controls {
    pub select: bool,
    pub auto_feed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlEvent {
    Changed(Controls),
    Initialize,
}

#[cfg(test)]
mod tests;
