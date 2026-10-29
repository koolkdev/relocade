//! Main-thread window lifecycle and graphics-worker hosting.
mod graphics;
mod host;
#[cfg_attr(not(target_arch = "wasm32"), path = "platform/native.rs")]
#[cfg_attr(target_arch = "wasm32", path = "platform/web.rs")]
mod platform;
mod session;
#[cfg(target_arch = "wasm32")]
pub mod web;

use crate::Application;
use futures_channel::oneshot;
use std::{
    future::Future,
    pin::Pin,
    task::{Context as TaskContext, Poll},
};
use winit::{
    event::KeyEvent,
    event_loop::{EventLoop, EventLoopProxy},
    keyboard::ModifiersState,
};

/// Window input delivered to the application's execution owner.
#[derive(Debug)]
pub enum Input {
    Key(KeyEvent),
    Modifiers(ModifiersState),
    Focused(bool),
}

/// Window-specific content for an application. The host owns the surface;
/// drawing records and submits commands using the supplied GPU handles.
pub trait WindowApplication: Application<Input = Input> {
    type Graphics: 'static;
    fn create_graphics(
        device: wgpu::Device,
        queue: wgpu::Queue,
        format: wgpu::TextureFormat,
    ) -> Result<Self::Graphics, String>;
    fn draw(
        graphics: &mut Self::Graphics,
        source: &Self::Source,
        attachment: &wgpu::TextureView,
        size: [u32; 2],
    ) -> Result<(), String>;
}

pub struct WindowOptions {
    pub title: String,
    /// Initial native logical size. Browser canvas layout comes from embedding CSS.
    pub size: [u32; 2],
}
impl Default for WindowOptions {
    fn default() -> Self {
        Self {
            title: "Relocade".into(),
            size: [960, 720],
        }
    }
}

/// Host lifetime. Completion includes worker cleanup and window/canvas removal.
/// Application completion retains the last source; close the window or call stop.
pub struct Control<T> {
    proxy: EventLoopProxy<host::Event>,
    finished: oneshot::Receiver<Result<Option<T>, String>>,
}
impl<T> Control<T> {
    pub fn stop(&self) {
        let _ = self.proxy.send_event(host::Event::Stop);
    }
}
impl<T> Future for Control<T> {
    type Output = Result<Option<T>, String>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.finished).poll(cx).map(|result| {
            result
                .unwrap_or_else(|_| Err("Window event loop ended before shutdown completed".into()))
        })
    }
}

/// Call on the application main thread. Native winit runs until the window closes;
/// web winit registers callbacks and returns immediately. Await the returned control
/// for shutdown and the retained application result (None if execution was canceled).
pub fn open<A: WindowApplication>(
    application: A,
    options: WindowOptions,
) -> Result<Control<A::Output>, String> {
    let event_loop = EventLoop::<host::Event>::with_user_event()
        .build()
        .map_err(|e| e.to_string())?;
    let proxy = event_loop.create_proxy();
    let (finished, completion) = oneshot::channel();
    let host = host::Host::new(
        session::Session::new(application, finished),
        options,
        proxy.clone(),
    );
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut host = host;
        event_loop.run_app(&mut host).map_err(|e| e.to_string())?;
    }
    #[cfg(target_arch = "wasm32")]
    {
        use winit::platform::web::EventLoopExtWebSys;
        event_loop.spawn_app(host);
    }
    Ok(Control {
        proxy,
        finished: completion,
    })
}
