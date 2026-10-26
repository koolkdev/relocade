use super::Post;
use crate::{backend::js_error, Error, Result};
use futures_channel::mpsc;
use futures_util::StreamExt;
use js_sys::Array;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{cell::Cell, marker::PhantomData};
use wasm_bindgen::{closure::Closure, JsCast, JsValue};
use web_sys::{MessageChannel, MessageEvent, MessagePort};

/// An unbounded, ordered channel between worker tasks.
/// Send each endpoint through Thread::run or another channel before receiving.
/// A receiver can be transferred only before its first recv call.
pub fn channel<T: Post>() -> Result<(Sender<T>, Receiver<T>)> {
    let channel =
        MessageChannel::new().map_err(|error| js_error("create worker channel", error))?;
    Ok((
        Sender::new(Port::new(channel.port1())),
        Receiver::new(Port::new(channel.port2())),
    ))
}

struct Port {
    port: MessagePort,
    handed_off: Cell<bool>,
}

impl Port {
    fn new(port: MessagePort) -> Self {
        Self {
            port,
            handed_off: Cell::new(false),
        }
    }

    fn transferables(&self) -> Array {
        // Thread::run drops its Rust context before its deferred postMessage.
        // The serialized JS value now owns the port until that transfer occurs.
        self.handed_off.set(true);
        Array::of1(&self.port)
    }

    fn check_local(&self) -> Result<()> {
        if self.handed_off.get() {
            Err(Error::new(
                "use worker channel",
                "endpoint has been handed to the transport",
            ))
        } else {
            Ok(())
        }
    }
}

impl Serialize for Port {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serde_wasm_bindgen::preserve::serialize(&self.port, serializer)
    }
}

impl<'de> Deserialize<'de> for Port {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        serde_wasm_bindgen::preserve::deserialize(deserializer).map(Self::new)
    }
}

/// The sending endpoint of a worker channel. Dropping it ends the stream.
#[derive(Serialize, Deserialize)]
#[serde(transparent, bound = "")]
pub struct Sender<T> {
    port: Port,
    #[serde(skip)]
    message: PhantomData<fn(T)>,
}

impl<T> Sender<T> {
    fn new(port: Port) -> Self {
        Self {
            port,
            message: PhantomData,
        }
    }
}

impl<T: Post> Sender<T> {
    /// Enqueues a message. Success does not acknowledge its receipt or detect a
    /// closed peer. Posting consumes transferred values, including on failure.
    pub fn send(&self, value: T) -> Result<()> {
        self.port.check_local()?;
        let frame = Array::of1(
            &value
                .to_js()
                .map_err(|error| js_error("encode worker message", error))?,
        );
        self.port
            .port
            .post_message_with_transferable(&frame, &value.transferables())
            .map_err(|error| js_error("send worker message", error))
    }
}

impl<T: Post> Post for Sender<T> {
    fn transferables(&self) -> Array {
        self.port.transferables()
    }
}

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        if !self.port.handed_off.get() {
            let _ = self.port.port.post_message(&JsValue::NULL);
            self.port.port.close();
        }
    }
}

/// The receiving endpoint of a worker channel.
/// Transfer it before the first recv call; receiving attaches a local inbox.
pub struct Receiver<T> {
    port: Port,
    inbox: Option<Inbox>,
    closed: bool,
    message: PhantomData<fn() -> T>,
}

impl<T> Receiver<T> {
    fn new(port: Port) -> Self {
        Self {
            port,
            inbox: None,
            closed: false,
            message: PhantomData,
        }
    }
}

impl<T: Post> Receiver<T> {
    /// Receives the next message, or None after the sender has been dropped.
    /// Cancelling this future preserves messages in this receiver's inbox.
    pub async fn recv(&mut self) -> Result<Option<T>> {
        self.port.check_local()?;
        if self.closed {
            return Ok(None);
        }
        let inbox = self
            .inbox
            .get_or_insert_with(|| Inbox::new(&self.port.port));
        match inbox.messages.next().await {
            Some(Ok(Some(value))) => T::from_js(value)
                .map(Some)
                .map_err(|error| js_error("decode worker message", error)),
            Some(Err(error)) => Err(error),
            Some(Ok(None)) | None => {
                self.closed = true;
                Ok(None)
            }
        }
    }
}

impl<T> Serialize for Receiver<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        if self.inbox.is_some() {
            return Err(serde::ser::Error::custom(
                "transfer a receiver before receiving messages",
            ));
        }
        self.port.serialize(serializer)
    }
}

impl<'de, T> Deserialize<'de> for Receiver<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        Port::deserialize(deserializer).map(Self::new)
    }
}

impl<T: Post> Post for Receiver<T> {
    fn transferables(&self) -> Array {
        if self.inbox.is_some() {
            // Serialization rejects an active inbox. Retain normal Drop cleanup.
            Array::new()
        } else {
            self.port.transferables()
        }
    }
}

impl<T> Drop for Receiver<T> {
    fn drop(&mut self) {
        if !self.port.handed_off.get() {
            self.port.port.set_onmessage(None);
            self.port.port.set_onmessageerror(None);
            self.port.port.close();
        }
    }
}

struct Inbox {
    messages: mpsc::UnboundedReceiver<Result<Option<JsValue>>>,
    _message: Closure<dyn FnMut(MessageEvent)>,
    _error: Closure<dyn FnMut(MessageEvent)>,
}

impl Inbox {
    fn new(port: &MessagePort) -> Self {
        let (sender, messages) = mpsc::unbounded();
        let errors = sender.clone();
        let message = Closure::new(move |event: MessageEvent| {
            let value = event.data();
            let frame = if value.is_null() {
                Ok(None)
            } else if let Some(array) = value.dyn_ref::<Array>().filter(|array| array.length() == 1)
            {
                Ok(Some(array.get(0)))
            } else {
                Err(Error::new(
                    "receive worker message",
                    "invalid channel frame",
                ))
            };
            let _ = sender.unbounded_send(frame);
        });
        let error = Closure::new(move |_: MessageEvent| {
            let _ = errors.unbounded_send(Err(Error::new(
                "receive worker message",
                "browser could not deserialize message",
            )));
        });
        port.set_onmessage(Some(message.as_ref().unchecked_ref()));
        port.set_onmessageerror(Some(error.as_ref().unchecked_ref()));
        Self {
            messages,
            _message: message,
            _error: error,
        }
    }
}
