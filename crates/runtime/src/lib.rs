//! Asynchronous Wasmtime execution with explicit protected-snapshot requests.
//!
//! [`Runtime::new`] queues interpreter generation and returns without waiting.
//! Workers own Wasm generation and engine compilation. [`Runtime::run_slice`]
//! polls completions, installs modules between invocations, and executes finite
//! guest work. [`HostMemory`] owns code registration, capture and memory coherence
//! in the Store; dispatch uses valid tickets without inspecting instruction bytes.

#![forbid(unsafe_code)]

mod host;
mod installation;
#[cfg(test)]
mod tests;
mod worker;

pub use host::{HostMemory, HostState};
pub use installation::CompiledEntry;
use std::{collections::HashMap, fmt, sync::mpsc::TryRecvError};
pub use wasm86_code_cache::{CodeRange, Mapping, Ticket};
pub use wasm86_codegen::Profile;
use wasm86_codegen::{Entry, Request, MAX_INSTRUCTIONS};
use wasm86_x86::SLICE_EXHAUSTED;
use wasmtime::{Linker, Memory, MemoryType, Store, TypedFunc};
use worker::{Job, Worker, MAX_PENDING};

const DISPATCH: u64 = 1024 << 48;
const INTERPRET: u64 = 2048 << 48;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SubmitError {
    Full,
    Stopped,
    InvalidRequest,
    UnavailableCode,
}
impl fmt::Display for SubmitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Full => "compilation queue is full",
            Self::Stopped => "compilation worker stopped",
            Self::InvalidRequest => "instruction limit is outside 1..=256",
            Self::UnavailableCode => "no readable code under the current execution context",
        })
    }
}
impl std::error::Error for SubmitError {}

#[derive(Debug)]
pub enum CompilationEvent {
    Installed { id: u64 },
    Discarded { id: u64 },
    Failed { id: u64, error: String },
}
#[derive(Debug, Eq, PartialEq)]
pub enum SliceExit {
    Starting,
    Yielded,
    /// Raw guest fault or unsupported-instruction exit from the shared ABI.
    Guest(u64),
    IncompatibleProfile,
    Unavailable,
}
pub struct Slice {
    pub exit: SliceExit,
    pub compilations: Vec<CompilationEvent>,
}

/// One execution owner, one fixed profile, and one background compiler.
pub struct Runtime<T> {
    store: Store<HostState<T>>,
    linker: Linker<HostState<T>>,
    memory: HostMemory,
    budget: Memory,
    worker: Worker,
    pending: HashMap<u64, Option<Ticket>>,
    blocks: HashMap<Ticket, TypedFunc<(), i64>>,
    interpreter: Option<TypedFunc<(), i64>>,
    interpret_next: bool,
    startup_failed: bool,
    stopped: bool,
}
impl<T: 'static> Runtime<T> {
    /// The linker supplies device and descriptor imports. The runtime defines
    /// the memory set, write invalidation, dispatch, handoff and slice budget.
    pub fn new(
        store: Store<HostState<T>>,
        linker: Linker<HostState<T>>,
        memory: HostMemory,
    ) -> wasmtime::Result<Self> {
        let worker = Worker::start(store.engine().clone())?;
        Self::with_worker(store, linker, memory, worker)
    }

    fn with_worker(
        mut store: Store<HostState<T>>,
        mut linker: Linker<HostState<T>>,
        memory: HostMemory,
        worker: Worker,
    ) -> wasmtime::Result<Self> {
        memory.define(&store, &mut linker)?;
        let budget = Memory::new(&mut store, MemoryType::new(1, None))?;
        linker.define(&store, "wasm86", "executionBudget", budget)?;
        linker.func_wrap("wasm86", "dispatch", |_: i32| DISPATCH as i64)?;
        linker.func_wrap("wasm86", "interpret", || INTERPRET as i64)?;
        worker.submit(Job {
            id: 0,
            request: Request {
                profile: memory.profile(),
                entry: Entry::Interpreter,
            },
        })?;
        Ok(Self {
            store,
            linker,
            memory,
            budget,
            worker,
            pending: HashMap::from([(0, None)]),
            blocks: HashMap::new(),
            interpreter: None,
            interpret_next: false,
            startup_failed: false,
            stopped: false,
        })
    }

    pub fn store(&self) -> &Store<HostState<T>> {
        &self.store
    }
    /// Use `memory()` for guest writes/remaps so host mutations invalidate code.
    pub fn store_mut(&mut self) -> &mut Store<HostState<T>> {
        &mut self.store
    }
    pub fn memory(&self) -> HostMemory {
        self.memory
    }

    /// Releases watches and scheduling while retaining memories and host payload.
    /// A new owner may adopt those memory handles under another execution profile.
    pub fn into_store(mut self) -> Store<HostState<T>> {
        self.memory.detach(&mut self.store);
        self.store
    }

    /// Protects and copies live code, then queues generation/compilation without
    /// waiting. A replacement leaves installed code usable until it is accepted.
    pub fn request_block(&mut self, eip: u32, instruction_limit: u32) -> Result<u64, SubmitError> {
        if self.stopped {
            return Err(SubmitError::Stopped);
        }
        if self.pending.len() >= MAX_PENDING {
            return Err(SubmitError::Full);
        }
        if !(1..=MAX_INSTRUCTIONS).contains(&instruction_limit) {
            return Err(SubmitError::InvalidRequest);
        }
        let (ticket, request) = self
            .memory
            .capture(&mut self.store, eip, instruction_limit)
            .ok_or(SubmitError::UnavailableCode)?;
        let id = ticket.id();
        if let Err(error) = self.worker.submit(Job { id, request }) {
            self.memory.cancel(&mut self.store, ticket);
            return Err(error);
        }
        self.pending.insert(id, Some(ticket));
        Ok(id)
    }

    fn install_ready(&mut self) -> Vec<CompilationEvent> {
        self.memory.enter(&mut self.store);
        let mut events = Vec::new();
        for _ in 0..MAX_PENDING {
            let completion = match self.worker.completions.try_recv() {
                Ok(completion) => completion,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.stopped = true;
                    self.startup_failed |= self.interpreter.is_none();
                    for (id, ticket) in self.pending.drain() {
                        if let Some(ticket) = ticket {
                            if !self.memory.is_pending(&self.store, ticket) {
                                events.push(CompilationEvent::Discarded { id });
                                continue;
                            }
                            self.memory.cancel(&mut self.store, ticket);
                        }
                        events.push(CompilationEvent::Failed {
                            id,
                            error: "compilation worker stopped".into(),
                        });
                    }
                    break;
                }
            };
            let id = completion.id;
            let Some(ticket) = self.pending.remove(&id) else {
                continue;
            };
            if ticket.is_some_and(|ticket| !self.memory.is_pending(&self.store, ticket)) {
                events.push(CompilationEvent::Discarded { id });
                continue;
            }
            let result = match completion.result {
                Ok(compiled) => match ticket {
                    Some(ticket) => self.install(ticket, compiled),
                    None => self.instantiate(compiled).map(|entry| {
                        self.interpreter = Some(entry);
                        true
                    }),
                }
                .map_err(|error| error.to_string()),
                Err(error) => {
                    if let Some(ticket) = ticket {
                        self.cancel_code(ticket);
                    }
                    Err(error)
                }
            };
            match result {
                Ok(true) => events.push(CompilationEvent::Installed { id }),
                Ok(false) => events.push(CompilationEvent::Discarded { id }),
                Err(error) => {
                    if ticket.is_none() {
                        self.startup_failed = true;
                    }
                    events.push(CompilationEvent::Failed { id, error });
                }
            }
        }
        self.blocks
            .retain(|ticket, _| self.memory.contains(&self.store, *ticket));
        events
    }

    /// Installs ready modules and executes at most `work` units without waiting
    /// for compilation. Traps are host errors; architectural exits remain values.
    pub fn run_slice(&mut self, work: u32) -> wasmtime::Result<Slice> {
        let compilations = self.install_ready();
        let exit = self.execute_slice(work)?;
        Ok(Slice { exit, compilations })
    }

    fn execute_slice(&mut self, work: u32) -> wasmtime::Result<SliceExit> {
        let Some(interpreter) = self.interpreter.clone() else {
            return Ok(if self.startup_failed {
                SliceExit::Unavailable
            } else {
                SliceExit::Starting
            });
        };
        self.budget.write(&mut self.store, 0, &work.to_le_bytes())?;
        loop {
            if u32::from_le_bytes(self.budget.data(&self.store)[..4].try_into().unwrap()) == 0 {
                return Ok(SliceExit::Yielded);
            }
            let Some((_, ticket)) = self.memory.enter(&mut self.store) else {
                return Ok(SliceExit::IncompatibleProfile);
            };
            let entry = if std::mem::take(&mut self.interpret_next) {
                interpreter.clone()
            } else {
                ticket
                    .and_then(|ticket| self.blocks.get(&ticket))
                    .cloned()
                    .unwrap_or_else(|| interpreter.clone())
            };
            match entry.call(&mut self.store, ())? as u64 {
                DISPATCH => {}
                INTERPRET => self.interpret_next = true,
                SLICE_EXHAUSTED => return Ok(SliceExit::Yielded),
                guest => return Ok(SliceExit::Guest(guest)),
            }
        }
    }
}
