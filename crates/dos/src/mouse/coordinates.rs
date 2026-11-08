//! DOS logical units, reporting granularity and conversion to drawing coordinates.
use super::state::Point;
use relocade_vga::FramebufferGeometry;

#[derive(Clone, Copy)]
pub(super) struct Coordinates {
    pub size: [i32; 2],
    pub step: [i32; 2],
}

impl Coordinates {
    pub fn new(geometry: Option<FramebufferGeometry>) -> Self {
        match geometry {
            Some(FramebufferGeometry::Text { columns, rows }) => {
                let step = [if columns == 40 { 16 } else { 8 }, 8];
                Self {
                    size: [i32::from(columns) * step[0], i32::from(rows) * step[1]],
                    step,
                }
            }
            Some(FramebufferGeometry::Graphics { width, height }) => {
                let step = [if width == 320 { 2 } else { 1 }, 1];
                Self {
                    size: [i32::from(width) * step[0], i32::from(height)],
                    step,
                }
            }
            None => Self {
                size: [640, 200],
                step: [1, 1],
            },
        }
    }

    pub fn drawing_position(self, point: Point) -> [i32; 2] {
        [
            i32::from(point.x as i16) / self.step[0],
            i32::from(point.y as i16) / self.step[1],
        ]
    }
}
