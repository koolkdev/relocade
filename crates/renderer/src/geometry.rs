use crate::Image;

/// A rectangle in output-attachment pixels. Extents are nonnegative; fractional
/// coordinates are supported. Drawing requires the rectangle to fit the attachment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub fn from_size(size: [u32; 2]) -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            width: size[0] as f32,
            height: size[1] as f32,
        }
    }
}

/// Centers the image within `available`, preserving its pixel aspect ratio.
/// Reads only image dimensions and aspect ratio; records no GPU work.
pub fn fit_image(image: &Image, available: Rect) -> Rect {
    let [width, height] = image.size();
    let (pixel_width, pixel_height) = image.pixel_aspect_ratio();
    let ratio =
        f64::from(width) * f64::from(pixel_width) / (f64::from(height) * f64::from(pixel_height));
    let width = f64::from(available.width).min(f64::from(available.height) * ratio);
    let height = f64::from(available.height).min(f64::from(available.width) / ratio);
    Rect {
        x: available.x + ((f64::from(available.width) - width) / 2.0) as f32,
        y: available.y + ((f64::from(available.height) - height) / 2.0) as f32,
        width: width as f32,
        height: height as f32,
    }
}
