use super::ChannelError;
use crate::{AsJs, Post, PostExt};
use futures_channel::mpsc;
use futures_util::StreamExt;
use js_sys::Array;
use std::{
    marker::PhantomData,
    task::{Context, Poll},
};
use wasm_bindgen::{closure::Closure, JsCast, JsValue};
use web_sys::{MessageChannel, MessageEvent, MessagePort};

// The worker transport can post this handle after its Rust wrapper is dropped.
// Only the receiver closes the connection; payload conversion must not close it.
pub struct Port<P> {
    port: MessagePort,
    marker: PhantomData<fn() -> P>,
}

pub type Sender<P> = Port<P>;
pub type ReceiverEndpoint<P> = Port<P>;

pub struct Receiver<P> {
    port: MessagePort,
    incoming: mpsc::UnboundedReceiver<Result<P, ChannelError>>,
    _messages: Closure<dyn FnMut(MessageEvent)>,
    _errors: Closure<dyn FnMut(MessageEvent)>,
}

pub fn channel<P: Post + 'static>() -> Result<(Sender<P>, ReceiverEndpoint<P>), ChannelError> {
    let channel = MessageChannel::new().map_err(diagnostic)?;
    Ok((
        Port {
            port: channel.port1(),
            marker: PhantomData,
        },
        Port {
            port: channel.port2(),
            marker: PhantomData,
        },
    ))
}

impl<P: Post + 'static> Port<P> {
    pub(super) fn bind(self) -> Receiver<P> {
        let port = self.port;
        let (messages, incoming) = mpsc::unbounded();
        let errors = messages.clone();
        let errors = Closure::<dyn FnMut(MessageEvent)>::new(move |_| {
            let _ = errors.unbounded_send(Err(ChannelError::Transport(
                "could not deserialize a worker message".into(),
            )));
        });
        let messages = Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
            let _ = messages.unbounded_send(P::from_js(event.data()).map_err(diagnostic));
        });
        port.set_onmessage(Some(messages.as_ref().unchecked_ref()));
        port.set_onmessageerror(Some(errors.as_ref().unchecked_ref()));
        port.start();
        Receiver {
            port,
            incoming,
            _messages: messages,
            _errors: errors,
        }
    }
}

impl<P: Post> Port<P> {
    pub(super) fn send(&self, payload: P) -> Result<(), ChannelError> {
        self.port.post(payload).map_err(diagnostic)
    }
}

impl<P> AsJs for Port<P> {
    fn to_js(&self) -> Result<JsValue, JsValue> {
        Ok(self.port.clone().into())
    }

    fn from_js(value: JsValue) -> Result<Self, JsValue> {
        Ok(Self {
            port: value.dyn_into()?,
            marker: PhantomData,
        })
    }
}

impl<P> Post for Port<P> {
    fn transferables(&self) -> Array {
        Array::of1(self.port.as_ref())
    }
}

impl<P> Receiver<P> {
    pub(super) fn poll_recv(&mut self, cx: &mut Context<'_>) -> Poll<Result<P, ChannelError>> {
        self.incoming
            .poll_next_unpin(cx)
            .map(|payload| payload.unwrap_or(Err(ChannelError::Closed)))
    }
}

impl<P> Drop for Receiver<P> {
    fn drop(&mut self) {
        self.port.set_onmessage(None);
        self.port.set_onmessageerror(None);
        self.port.close();
    }
}

fn diagnostic(error: JsValue) -> ChannelError {
    ChannelError::Transport(format!("{error:?}"))
}
