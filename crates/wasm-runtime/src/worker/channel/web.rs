use super::Post;
use crate::{Error, Result};
use futures_channel::mpsc;
use futures_util::StreamExt;
use js_sys::Array;
use std::{
    marker::PhantomData,
    task::{Context, Poll},
};
use wasm_bindgen::{closure::Closure, JsCast, JsValue};
use web_sys::{MessageChannel, MessageEvent, MessagePort};

/// A resource channel endpoint that can move to its final worker before binding.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(transparent, bound = "")]
pub struct Endpoint<T> {
    #[serde(with = "serde_wasm_bindgen::preserve")]
    port: MessagePort,
    #[serde(skip)]
    marker: PhantomData<fn() -> T>,
}

/// A bound port for sending and receiving engine resources.
pub struct Port<T> {
    port: MessagePort,
    messages: mpsc::UnboundedReceiver<Result<T>>,
    _listener: Closure<dyn FnMut(MessageEvent)>,
}

/// Creates two transferable endpoints. Bind each on its final owner.
pub fn channel<T: Post + 'static>() -> Result<(Endpoint<T>, Endpoint<T>)> {
    let channel =
        MessageChannel::new().map_err(|error| diagnostic("create worker channel", error))?;
    Ok((
        Endpoint::new(channel.port1()),
        Endpoint::new(channel.port2()),
    ))
}

impl<T: Post + 'static> Endpoint<T> {
    fn new(port: MessagePort) -> Self {
        Self {
            port,
            marker: PhantomData,
        }
    }

    /// Binds this endpoint to its receiving owner. The resulting port is local;
    /// its listener and Rust inbox cannot be posted to another worker.
    pub fn bind(self) -> Port<T> {
        let (messages, incoming) = mpsc::unbounded();
        let mut messages = Some(messages);
        let listener = Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
            let packet: Array = event.data().unchecked_into();
            if packet.length() == 0 {
                messages.take();
            } else if let Some(messages) = &messages {
                let value = T::from_js(packet.get(0))
                    .map_err(|error| diagnostic("receive worker resource", error));
                let _ = messages.unbounded_send(value);
            }
        });
        self.port
            .set_onmessage(Some(listener.as_ref().unchecked_ref()));
        self.port.start();
        Port {
            port: self.port,
            messages: incoming,
            _listener: listener,
        }
    }
}

impl<T: Post + 'static> Port<T> {
    /// Queues a resource for the peer without waiting for it to receive.
    pub fn send(&self, value: T) -> Result<()> {
        let message = value
            .to_js()
            .map_err(|error| diagnostic("encode worker resource", error))?;
        self.port
            .post_message_with_transferable(&Array::of1(&message), &value.transferables())
            .map_err(|error| diagnostic("send worker resource", error))
    }

    /// Polls delivery and registers the caller for the next value or normal close.
    pub fn poll_recv(&mut self, cx: &mut Context<'_>) -> Poll<Option<Result<T>>> {
        self.messages.poll_next_unpin(cx)
    }
}

impl<T> Drop for Port<T> {
    fn drop(&mut self) {
        // Bound ports are never transferred. An empty packet closes the peer's
        // stream after all earlier messages before releasing this local listener.
        let _ = self.port.post_message(&Array::new());
        self.port.set_onmessage(None);
        self.port.close();
    }
}

impl<T: Post + 'static> Post for Endpoint<T> {
    fn transferables(&self) -> Array {
        Array::of1(self.port.as_ref())
    }
}

fn diagnostic(operation: &'static str, error: JsValue) -> Error {
    Error::new(operation, format!("{error:?}"))
}
