mod message;
pub(crate) mod requests;
pub use message::Packet;
pub use wasm_bindgen::JsValue as Value;

#[doc(hidden)]
pub mod __private {
    pub use super::message::{decode_fields, encode_fields};
}

use crate::{Error, Result};
use js_sys::{Array, Promise};
use std::future::Future;
use wasm_bindgen::{prelude::*, JsCast};
use wasm_bindgen_futures::JsFuture;

#[wasm_bindgen(module = "/src/web.js")]
extern "C" {
    type WorkerOwner;
    #[wasm_bindgen(constructor, catch)]
    fn new(
        bindings: &str,
        wasm: &str,
        entry: &str,
        configuration: &JsValue,
    ) -> std::result::Result<WorkerOwner, JsValue>;
    #[wasm_bindgen(method, catch, js_name = send)]
    fn post(
        this: &WorkerOwner,
        value: &JsValue,
        transfer: &Array,
    ) -> std::result::Result<(), JsValue>;
    #[wasm_bindgen(method)]
    fn shutdown(this: &WorkerOwner);
    #[wasm_bindgen(method, getter)]
    fn readiness(this: &WorkerOwner) -> Promise;
    #[wasm_bindgen(method, getter)]
    fn completion(this: &WorkerOwner) -> Promise;
    #[wasm_bindgen(method, getter)]
    fn events(this: &WorkerOwner) -> Inbox;
    #[wasm_bindgen(method, getter, js_name = inputOpen)]
    fn input_open(this: &WorkerOwner) -> bool;
}

#[wasm_bindgen]
extern "C" {
    type Inbox;
    #[wasm_bindgen(method)]
    fn available(this: &Inbox) -> Promise;
    #[wasm_bindgen(method, catch)]
    fn take(this: &Inbox) -> std::result::Result<Array, JsValue>;

    /// Worker-local endpoint supplied to a Wasm entry export.
    pub type Endpoint;
    #[wasm_bindgen(method)]
    pub fn ready(this: &Endpoint);
    #[wasm_bindgen(method, catch, js_name = send)]
    fn post(this: &Endpoint, value: &JsValue, transfer: &Array)
        -> std::result::Result<(), JsValue>;
}

/// Owns submission to a browser worker. Dropping it closes input gracefully.
pub struct Worker(WorkerOwner);

/// Receives events independently of the submission owner.
pub struct Receiver(Inbox);

impl Worker {
    /// Loads a worker-safe `wasm-bindgen --target web` build from absolute URLs.
    /// The named async entry receives `(bindings, endpoint, configuration)`.
    /// It calls `endpoint.ready()` after initialization, processes commands,
    /// then returns after input closes. Configuration is structured-cloned.
    pub fn spawn(
        bindings: &str,
        wasm: &str,
        entry: &str,
        configuration: &JsValue,
    ) -> Result<(Self, Receiver)> {
        let worker = WorkerOwner::new(bindings, wasm, entry, configuration).map_err(failed)?;
        let events = Receiver(worker.events());
        Ok((Self(worker), events))
    }

    /// Structured-clones a message. Transfer lists explicitly move owned buffers
    /// or ports; shared memories and compiled Wasm modules are cloned, not transferred.
    pub fn send(&self, message: &JsValue, transfer: &[JsValue]) -> Result<()> {
        if !self.0.input_open() {
            return Err(Error::Closed);
        }
        self.0
            .post(message, &transfer.iter().collect())
            .map_err(failed)
    }

    pub fn ready(&self) -> impl Future<Output = Result<()>> + 'static {
        completion(self.0.readiness())
    }

    /// Closes input. The worker processes accepted commands and returns normally.
    pub fn shutdown(&mut self) {
        self.0.shutdown();
    }

    pub fn stopped(&self) -> impl Future<Output = Result<()>> + 'static {
        completion(self.0.completion())
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl Receiver {
    /// A cancelled receive leaves the next message available. None means the
    /// entry returned successfully; terminal errors are returned after queued events.
    pub async fn recv(&mut self) -> Result<Option<JsValue>> {
        receive(&self.0).await
    }
}

impl Endpoint {
    pub async fn recv(&mut self) -> Result<Option<JsValue>> {
        receive(self.unchecked_ref()).await
    }

    pub fn send(&self, message: &JsValue, transfer: &[JsValue]) -> Result<()> {
        self.post(message, &transfer.iter().collect())
            .map_err(failed)
    }
}

async fn receive(inbox: &Inbox) -> Result<Option<JsValue>> {
    JsFuture::from(inbox.available()).await.map_err(failed)?;
    let result = inbox.take().map_err(failed)?;
    Ok(result.get(0).as_bool().unwrap().then(|| result.get(1)))
}

async fn completion(promise: Promise) -> Result<()> {
    JsFuture::from(promise).await.map(|_| ()).map_err(failed)
}

fn failed(error: JsValue) -> Error {
    Error::Failed(error.as_string().unwrap_or_else(|| format!("{error:?}")))
}
