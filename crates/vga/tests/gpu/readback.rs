//! Texture readback shared by VGA and guest-to-GPU integration tests.

/// Observe the active picture at a fixture's row sampling rate. Pixel-decoding
/// fixtures remain independent of the surrounding border; overscan fixtures read
/// the complete texture. Active positions removed by blanking read as black.
pub(crate) async fn read_active_pixels(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    snapshot: &relocade_vga::DisplaySnapshot,
    size: [u32; 2],
) -> Vec<u8> {
    let pixels = read_pixels(device, queue, texture).await;
    let geometry = snapshot.geometry();
    assert_eq!([texture.width(), texture.height()], geometry.size);
    assert_eq!(geometry.active_size[0], size[0]);
    let c = &snapshot.registers.crtc.values;
    let clock_width = if snapshot.registers.sequencer.values[1] & 1 == 0 {
        9
    } else {
        8
    };
    let pixel_step = if snapshot.registers.attribute.values[0x10] & 0x41 == 0x41
        && snapshot.registers.graphics.values[5] & 0x40 != 0
    {
        2
    } else {
        1
    };
    let vertical_divisor = 1 << ((c[0x17] >> 2) & 1);
    let total = [
        (u32::from(c[0]) + 5) * clock_width,
        (u32::from(c[6]) + (u32::from(c[7] & 1) << 8) + (u32::from(c[7] & 0x20) << 4) + 2)
            * vertical_divisor,
    ];
    let skew = u32::from((c[3] >> 5) & 3) * clock_width;
    let physical_height = geometry.active_size[1];
    assert_eq!(physical_height % size[1], 0);
    let row_step = physical_height / size[1];
    let mut active = Vec::with_capacity((size[0] * size[1] * 4) as usize);
    for y in 0..size[1] {
        for x in 0..size[0] {
            let column = ((x * pixel_step + skew + total[0] - geometry.raster_origin[0])
                % total[0])
                / pixel_step;
            let row = (y * row_step + total[1] - geometry.raster_origin[1]) % total[1];
            if column < geometry.size[0] && row < geometry.size[1] {
                let offset = ((row * geometry.size[0] + column) * 4) as usize;
                active.extend_from_slice(&pixels[offset..offset + 4]);
            } else {
                active.extend_from_slice(&[0, 0, 0, 255]);
            }
        }
    }
    active
}

pub(crate) async fn read_pixels(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
) -> Vec<u8> {
    let row_bytes = texture.width() * 4;
    let stride =
        row_bytes.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("VGA test readback"),
        size: u64::from(stride) * u64::from(texture.height()),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(stride),
                rows_per_image: Some(texture.height()),
            },
        },
        texture.size(),
    );
    queue.submit([encoder.finish()]);
    let (send, receive) = futures_channel::oneshot::channel();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            send.send(result).unwrap();
        });
    #[cfg(not(target_arch = "wasm32"))]
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    receive.await.unwrap().unwrap();
    let mapped = buffer.slice(..).get_mapped_range().unwrap();
    let pixels = mapped
        .chunks_exact(stride as usize)
        .flat_map(|row| row[..row_bytes as usize].iter().copied())
        .collect();
    drop(mapped);
    buffer.unmap();
    pixels
}
