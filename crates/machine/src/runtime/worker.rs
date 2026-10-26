//! Rust workers and typed channels for platform objects and ordinary Rust values.

pub use relocade_wasm_runtime::worker::{
    channel, Error, Message, Post, Receiver, SendTask, Sender, Task, Thread,
};
