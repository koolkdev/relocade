//! A main-thread DOS window with separate execution and graphics workers.
#![forbid(unsafe_code)]

mod application;
mod execution;
mod graphics;
mod keyboard;
mod tasks;

#[cfg_attr(not(target_arch = "wasm32"), path = "platform/native.rs")]
#[cfg_attr(target_arch = "wasm32", path = "platform/web.rs")]
mod platform;

use application::Application;
use std::{cell::RefCell, rc::Rc};

/// Runs a supplied COM or MZ program. Program exit retains its screen until close.
/// Native platforms wait for window shutdown; web registers callbacks and returns.
pub fn run(program: Vec<u8>) -> Result<(), String> {
    let error = Rc::new(RefCell::new(None));
    relocade_app::run(|events| Application::new(program, events, error.clone()))
        .map_err(|e| e.to_string())?;
    let result = error.borrow_mut().take().map_or(Ok(()), Err);
    result
}
