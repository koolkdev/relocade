//! Execution preparation, callable dispatch, and instruction budgets for configured guests.
//!
//! [`wasm86_machine::Machine`] supplies the CPU context, address space and environment.
//! [`Runtime::attach`] prepares code for its complete execution target and produces
//! a persistent [`Session`]. Backends own engine objects and memories. Preparation
//! is asynchronous so the same contract supports native and browser engines;
//! guest entry and environment callbacks are synchronous.
//!
//! ```
//! use wasm86_machine::{CpuContext, Machine};
//! use wasm86_runtime::{backend::Backend, Runtime};
//! use wasm86_x86::{Compiler, CpuState, ExecutionExit, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap, Segments};
//!
//! async fn example(backend: impl Backend) -> Result<(), Box<dyn std::error::Error>> {
//!     let target = Compiler::new(ExecutionProfile::Real16).target();
//!     let cpu = CpuState { eip: 0x100, segments: Segments::real_mode(), ..CpuState::default() };
//!     let context = CpuContext::new(target, cpu)?;
//!     let map = PhysicalMemoryMap::new([(0..=0xffff, PhysicalMapping::Ram { backing_offset: 0 })])?;
//!     let mut machine = Machine::new(context, map, ())?;
//!     machine.initialize_backing(0x100, &[0xb8, 42, 0, 0xeb, 0])?;
//!     let runtime = Runtime::new(backend);
//!     let mut session = runtime.attach(machine).await?;
//!     assert_eq!(session.run(1)?, ExecutionExit::BudgetExhausted);
//!     assert_eq!(session.cpu().registers.eax, 42);
//!     Ok(())
//! }
//! ```
#![forbid(unsafe_code)]

pub mod backend;
mod error;
mod host;
mod module;
mod run;
mod session;

pub use error::{Error, Result};
pub use module::ExecutableModule;
pub use session::Session;

use backend::Backend;
use std::{cell::RefCell, collections::HashMap, rc::Rc};
use wasm86_machine::{Environment, Machine};
use wasm86_x86::{Compiler, ExecutionTarget};

/// Shares prepared modules across sessions on one embedding thread. Neither
/// backend futures nor guest environment state are required to be Send.
pub struct Runtime<B: Backend> {
    code: Rc<RuntimeCode<B>>,
}
struct RuntimeCode<B: Backend> {
    backend: B,
    targets: RefCell<HashMap<ExecutionTarget, Rc<Prepared<B::Module>>>>,
}
struct Prepared<M> {
    interpreter: M,
    dispatcher: M,
}

impl<B: Backend> Runtime<B> {
    pub fn new(backend: B) -> Self {
        Self {
            code: Rc::new(RuntimeCode {
                backend,
                targets: RefCell::new(HashMap::new()),
            }),
        }
    }

    pub async fn attach<E: Environment + 'static>(
        &self,
        machine: Machine<E>,
    ) -> Result<Session<B, E>> {
        Session::new(self, machine).await
    }

    async fn prepare(&self, target: ExecutionTarget) -> Result<Rc<Prepared<B::Module>>> {
        if let Some(code) = self.code.targets.borrow().get(&target) {
            return Ok(code.clone());
        }
        let compiler = Compiler::new(target.profile).with_cpu_model(target.cpu_model);
        let interpreter = self
            .code
            .backend
            .compile(&compiler.compile_interpreter()?)
            .await?;
        let dispatcher = self
            .code
            .backend
            .compile(&compiler.compile_dispatcher()?)
            .await?;
        let prepared = Rc::new(Prepared {
            interpreter,
            dispatcher,
        });
        Ok(self
            .code
            .targets
            .borrow_mut()
            .entry(target)
            .or_insert(prepared)
            .clone())
    }
}
impl<B: Backend> Clone for Runtime<B> {
    fn clone(&self) -> Self {
        Self {
            code: self.code.clone(),
        }
    }
}
