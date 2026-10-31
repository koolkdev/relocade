use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use relocade_app::{winit::dpi::PhysicalSize, Window};
use std::time::{Duration, Instant};
use x11rb::{
    connection::Connection,
    protocol::xproto::{ConnectionExt, ImageFormat, ImageOrder},
};

pub fn window_id(window: &Window) -> u32 {
    match window.winit().window_handle().unwrap().as_raw() {
        RawWindowHandle::Xlib(handle) => handle.window as u32,
        RawWindowHandle::Xcb(handle) => handle.window.get(),
        other => panic!("X11 fixture received {other:?}"),
    }
}

pub fn presented_pixels_match(window: u32, size: PhysicalSize<u32>) -> bool {
    let (connection, _) = x11rb::connect(None).unwrap();
    let pixel = |x, y| {
        let image = connection
            .get_image(ImageFormat::Z_PIXMAP, window, x, y, 1, 1, u32::MAX)
            .unwrap()
            .reply()
            .unwrap();
        assert_eq!(image.depth, 24);
        let bytes = image.data[..4].try_into().unwrap();
        let value = if connection.setup().image_byte_order == ImageOrder::LSB_FIRST {
            u32::from_le_bytes(bytes)
        } else {
            u32::from_be_bytes(bytes)
        };
        [(value >> 16) as u8, (value >> 8) as u8, value as u8]
    };
    // A square image fits centrally; sample inside its red and white quadrants.
    let side = size.width.min(size.height);
    let left = (size.width - side) / 2;
    let top = (size.height - side) / 2;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if pixel((left + side / 4) as i16, (top + side / 4) as i16) == [255, 0, 0]
            && pixel((left + 3 * side / 4) as i16, (top + 3 * side / 4) as i16) == [255, 255, 255]
        {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
