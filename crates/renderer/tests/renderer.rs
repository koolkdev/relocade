#![cfg(not(target_arch = "wasm32"))]

use futures_channel::{mpsc, oneshot};
use futures_util::{future, StreamExt};
use relocade_renderer::{Graphics, Renderer, Texture};
use std::{
    future::Future,
    pin::Pin,
    task::{ready, Context, Poll},
};

struct Request {
    color: [u8; 4],
    reply: oneshot::Sender<[u8; 4]>,
}

struct ReadbackRenderer {
    requests: mpsc::UnboundedReceiver<Request>,
    graphics: Graphics,
    texture: Texture,
}

impl Renderer for ReadbackRenderer {
    type Source = mpsc::UnboundedReceiver<Request>;

    fn new(requests: Self::Source, graphics: Graphics) -> Result<Self, String> {
        let texture = Texture::new(&graphics.device, [1, 1]).unwrap();
        Ok(Self {
            requests,
            graphics,
            texture,
        })
    }

    fn texture(&self) -> Option<&Texture> {
        Some(&self.texture)
    }
}

impl Future for ReadbackRenderer {
    type Output = Result<(), String>;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        let Some(request) = ready!(this.requests.poll_next_unpin(context)) else {
            return Poll::Ready(Ok(()));
        };
        this.texture
            .write(&this.graphics.queue, &request.color)
            .unwrap();
        let buffer = this.graphics.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("texture readback without host frames"),
            size: 256,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = this
            .graphics
            .device
            .create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            this.texture.raw().as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(256),
                    rows_per_image: Some(1),
                },
            },
            this.texture.raw().size(),
        );
        this.graphics.queue.submit([encoder.finish()]);
        let (mapped, mut mapped_result) = oneshot::channel();
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                mapped.send(result).unwrap();
            });
        // This native fixture drives readback itself, without a window loop.
        this.graphics
            .device
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        mapped_result.try_recv().unwrap().unwrap().unwrap();
        let bytes = buffer.slice(..).get_mapped_range().unwrap();
        request.reply.send(bytes[..4].try_into().unwrap()).unwrap();
        drop(bytes);
        buffer.unmap();
        context.waker().wake_by_ref();
        Poll::Pending
    }
}

#[test]
fn requests_and_readback_progress_without_host_frames() {
    pollster::block_on(async {
        let instance = wgpu::Instance::default();
        let adapter = instance.request_adapter(&Default::default()).await.unwrap();
        let (device, queue) = adapter.request_device(&Default::default()).await.unwrap();
        let (requests, incoming) = mpsc::unbounded();
        let mut renderer = ReadbackRenderer::new(incoming, Graphics { device, queue }).unwrap();
        let exchange = async {
            // Each request awaits a GPU readback before sending the next one.
            // No viewer, surface or redraw participates in this exchange.
            for color in [[17, 85, 170, 255], [64, 32, 96, 17]] {
                let (reply, result) = oneshot::channel();
                requests.unbounded_send(Request { color, reply }).unwrap();
                assert_eq!(result.await.unwrap(), color);
            }
            drop(requests);
        };
        let (result, ()) = future::join(&mut renderer, exchange).await;
        result.unwrap();
        assert_eq!(renderer.texture().unwrap().size(), [1, 1]);
    });
}
