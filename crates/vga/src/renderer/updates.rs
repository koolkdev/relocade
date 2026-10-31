//! Image invalidation for the display adapter's submitted, retained image.
mod text;

use super::state;
use crate::DisplaySnapshot;
use std::sync::Arc;
use text::TextLayout;

/// Exclusive pixel bounds, clipped to the image before encoding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PixelRect {
    pub left: u32,
    pub top: u32,
    pub right: u32,
    pub bottom: u32,
}

impl PixelRect {
    pub fn full(size: [u32; 2]) -> Self {
        Self {
            left: 0,
            top: 0,
            right: size[0],
            bottom: size[1],
        }
    }

    pub fn include(region: &mut Option<Self>, rect: Self) {
        if let Some(region) = region {
            region.left = region.left.min(rect.left);
            region.top = region.top.min(rect.top);
            region.right = region.right.max(rect.right);
            region.bottom = region.bottom.max(rect.bottom);
        } else {
            *region = Some(rect);
        }
    }
}

#[derive(Default)]
pub(crate) struct ImageUpdates {
    previous: Option<DisplaySnapshot>,
    pending: Option<PixelRect>,
}

impl ImageUpdates {
    /// Called only when the renderer's existing upload checks report a change.
    pub fn include(&mut self, snapshot: &DisplaySnapshot) -> bool {
        if self
            .previous
            .as_ref()
            .is_some_and(|previous| previous.geometry().size != snapshot.geometry().size)
        {
            // The renderer replaced its output texture; old pixel coordinates
            // no longer describe that storage, even if drawing was deferred.
            self.pending = None;
        }
        let region = self.previous.as_ref().map_or_else(
            || Some(PixelRect::full(snapshot.geometry().size)),
            |previous| changed_region(previous, snapshot),
        );
        if let Some(region) = region {
            PixelRect::include(&mut self.pending, region);
        }
        let mut previous = snapshot.clone();
        // Retain the current planes, without retaining their earlier baseline.
        previous.changes = None;
        self.previous = Some(previous);
        region.is_some()
    }

    /// The display host submits each encoded image before servicing more inputs.
    pub fn take(&mut self) -> Option<PixelRect> {
        self.pending.take()
    }
}

fn changed_region(previous: &DisplaySnapshot, current: &DisplaySnapshot) -> Option<PixelRect> {
    let full = Some(PixelRect::full(current.geometry().size));
    let Some(layout) = TextLayout::new(current) else {
        return full;
    };
    if !state::same_layout(previous, current)
        || previous.registers.dac.colors != current.registers.dac.colors
    {
        return full;
    }
    let mut region = None;
    for plane in 0..4 {
        if Arc::ptr_eq(&previous.planes[plane], &current.planes[plane]) {
            continue;
        }
        // Font changes and unknown/skipped write intervals affect the whole image.
        let Some(changes) = current.changes.as_ref().filter(|changes| {
            plane < 2 && Arc::ptr_eq(&previous.planes[plane], &changes.baseline[plane])
        }) else {
            return full;
        };
        layout.include_bytes(&mut region, changes.writes.bounds(plane));
    }
    layout.include_blink_and_cursor(&mut region, previous, current);
    region
}
