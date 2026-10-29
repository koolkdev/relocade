use arc_swap::ArcSwapOption;
use event_listener::Event;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

/// Completed VGA output. Pixels are tightly packed, top-to-bottom RGBA8,
/// with sRGB color values and opaque alpha. Published frames are immutable.
#[derive(Debug)]
pub struct VideoFrame {
    pub width: u32,
    pub height: u32,
    pub pixels: Box<[u8]>,
    /// Pixel width / height. Combine with the image dimensions when sizing a viewport.
    pub pixel_aspect_ratio: (u32, u32),
    /// Elapsed session time at the completed frame boundary.
    pub time: Duration,
    /// Increasing frame identity; skipped frames can leave gaps.
    pub sequence: u64,
}

#[derive(Default)]
struct Output {
    frame: ArcSwapOption<VideoFrame>,
    changed: Event,
    closed: AtomicBool,
}

/// Read-only access to the latest completed VGA frame.
/// Clones can cross workers and outlive the device. There is one retained frame,
/// with no queue or producer backpressure. Previously retained frames stay stable.
#[derive(Clone)]
pub struct Display {
    output: Arc<Output>,
}

impl Display {
    pub(super) fn new() -> Self {
        Self {
            output: Arc::new(Output::default()),
        }
    }

    /// Returns the currently published completed frame.
    /// Returns `None` until the device has serviced its first frame deadline.
    /// Call from the application's own redraw schedule; reading or rendering a
    /// frame does not advance VGA time.
    pub fn current_frame(&self) -> Option<Arc<VideoFrame>> {
        self.output.frame.load_full()
    }

    /// Waits for a frame newer than `after`, skipping any obsolete frames.
    /// Pass `None` for the first frame, then the last received sequence. Multiple consumers can wait independently.
    /// Returns `None` once the device is gone and no newer frame remains.
    pub async fn next_frame(&self, after: Option<u64>) -> Option<Arc<VideoFrame>> {
        loop {
            let changed = self.output.changed.listen();
            // Observe closure before loading the last frame so a concurrent final
            // store cannot be mistaken for an exhausted, closed output.
            let closed = self.output.closed.load(Ordering::Acquire);
            if let Some(frame) = self.current_frame() {
                if after.is_none_or(|sequence| frame.sequence > sequence) {
                    return Some(frame);
                }
            }
            if closed {
                return None;
            }
            changed.await;
        }
    }

    pub(super) fn complete(&self, frame: VideoFrame) {
        self.output.frame.store(Some(Arc::new(frame)));
        self.output.changed.notify(usize::MAX);
    }

    pub(super) fn close(&self) {
        self.output.closed.store(true, Ordering::Release);
        self.output.changed.notify(usize::MAX);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::FutureExt;

    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn sequence_identifies_new_output_even_when_timestamps_match() {
        let display = Display::new();
        for sequence in [1, 2] {
            display.complete(VideoFrame {
                width: 1,
                height: 1,
                pixels: Box::new([0, 0, 0, 255]),
                pixel_aspect_ratio: (1, 1),
                time: Duration::ZERO,
                sequence,
            });
        }
        display.close();
        // A consumer still receives the last unseen frame after closure.
        let frame = display.next_frame(Some(1)).now_or_never().unwrap().unwrap();
        assert_eq!(frame.sequence, 2);
        assert!(display
            .next_frame(Some(2))
            .now_or_never()
            .unwrap()
            .is_none());
    }
}
