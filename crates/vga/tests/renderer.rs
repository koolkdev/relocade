#![cfg(feature = "renderer")]

use futures_channel::{mpsc, oneshot};
use futures_timer::Delay;
use futures_util::{future, StreamExt};
use relocade_machine::Device;
use relocade_renderer::{Graphics, Renderer, Texture};
use relocade_vga::{Vga, VgaRenderer, CRTC_INDEX_PORT, TEXT_MEMORY_START};
use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Wake},
    time::Duration,
};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

struct TimerWake(mpsc::UnboundedSender<()>);

impl Wake for TimerWake {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        let _ = self.0.unbounded_send(());
    }
}

async fn pixels(texture: &Texture, graphics: &Graphics) -> Vec<[u8; 4]> {
    let [width, height] = texture.size();
    let row_bytes = width * 4;
    let stride =
        row_bytes.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buffer = graphics.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("VGA texture readback"),
        size: u64::from(stride) * u64::from(height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = graphics.device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        texture.raw().as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(stride),
                rows_per_image: Some(height),
            },
        },
        texture.raw().size(),
    );
    graphics.queue.submit([encoder.finish()]);
    let (mapped, result) = oneshot::channel();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            mapped.send(result).unwrap();
        });
    #[cfg(not(target_arch = "wasm32"))]
    graphics
        .device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    result.await.unwrap().unwrap();
    let bytes = buffer.slice(..).get_mapped_range().unwrap();
    let pixels = bytes
        .chunks_exact(stride as usize)
        .flat_map(|row| row[..row_bytes as usize].as_chunks::<4>().0.iter().copied())
        .collect();
    drop(bytes);
    buffer.unmap();
    pixels
}

async fn check_renderer() {
    let instance = wgpu::Instance::default();
    let adapter = instance.request_adapter(&Default::default()).await.unwrap();
    let (device, queue) = adapter.request_device(&Default::default()).await.unwrap();
    let graphics = Graphics { device, queue };
    let mut vga = Vga::new();
    for cell in 0..2000 {
        vga.write_mmio(TEXT_MEMORY_START + cell * 2, 2, 0x0720);
    }
    vga.write_mmio(TEXT_MEMORY_START, 2, 0x1edb);
    for (index, value) in [(0x0a, 6), (0x0b, 7), (0x0e, 7), (0x0f, 0xcf)] {
        vga.write_port(CRTC_INDEX_PORT, 2, index | (value << 8));
    }
    vga.publish();
    let mut renderer = VgaRenderer::new(vga.display(), graphics.clone()).unwrap();
    assert_eq!(renderer.texture().unwrap().size(), [640, 200]);
    // Construction initializes the texture without a window or a first poll.
    let initial = pixels(renderer.texture().unwrap(), &graphics).await;
    assert_eq!(initial[0], [255, 255, 85, 255]);
    assert_eq!(initial[8], [0, 0, 0, 255]);
    assert_eq!(initial[640 * 200 - 1], [170, 170, 170, 255]);

    let (wake, mut timer_wakes) = mpsc::unbounded();
    let waker = Arc::new(TimerWake(wake)).into();
    let mut context = Context::from_waker(&waker);
    assert!(Pin::new(&mut renderer).poll(&mut context).is_pending());
    for (attribute, color) in [(0x4a, [85, 255, 85, 255]), (0x0c, [255, 85, 85, 255])] {
        vga.write_mmio(TEXT_MEMORY_START + 1, 1, attribute);
        vga.publish();
        let refreshed = async {
            loop {
                // Only the renderer's timer wakes this poll. A host redraw or the
                // timeout below cannot compensate for a missing timer registration.
                timer_wakes.next().await.unwrap();
                assert!(Pin::new(&mut renderer).poll(&mut context).is_pending());
                if pixels(renderer.texture().unwrap(), &graphics).await[0] == color {
                    break;
                }
            }
        };
        assert!(
            matches!(
                future::select(Box::pin(refreshed), Delay::new(Duration::from_secs(60))).await,
                future::Either::Left(_)
            ),
            "VGA refresh did not wake and upload the published screen"
        );
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn initializes_and_refreshes_the_display_without_host_frames() {
    pollster::block_on(check_renderer());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn initializes_and_refreshes_the_display_without_host_frames() {
    check_renderer().await;
}
