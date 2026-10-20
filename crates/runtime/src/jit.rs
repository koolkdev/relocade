//! Automatic hot-entry promotion through the bounded compiler queue.

use crate::{Result, Runtime, RuntimeBackend, RuntimeState};
use std::num::{NonZeroU16, NonZeroU32};

/// Compilation and cache bounds for automatic JIT execution. These bound work
/// and code ownership, not guest execution time or instruction-budget overshoot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JitConfig {
    pub hot_threshold: NonZeroU16,
    pub max_blocks: NonZeroU16,
    pub instructions_per_block: NonZeroU16,
    /// Maximum live managed groups, including explicitly installed groups.
    /// The oldest group is evicted first; fixed modules and hooks are preserved.
    pub max_groups: NonZeroU16,
    /// Lifetime accepted automatic compilation jobs, including failed, stale or
    /// untranslatable jobs. Engines such as Wasmtime retain instances until
    /// teardown; eviction alone cannot bound their resources. Exhaustion leaves
    /// execution in existing compiled entries or the interpreter.
    pub max_compilations: NonZeroU32,
}

impl Default for JitConfig {
    fn default() -> Self {
        Self {
            hot_threshold: NonZeroU16::new(32).unwrap(),
            max_blocks: NonZeroU16::new(8).unwrap(),
            instructions_per_block: NonZeroU16::new(32).unwrap(),
            max_groups: NonZeroU16::new(64).unwrap(),
            max_compilations: NonZeroU32::new(1024).unwrap(),
        }
    }
}

#[derive(Default)]
pub(super) struct JitState {
    pub config: JitConfig,
    compilations: u32,
}

impl JitState {
    pub(super) fn can_compile(&self) -> bool {
        self.compilations < self.config.max_compilations.get()
    }
}

impl<T: 'static, B: RuntimeBackend<State = RuntimeState<T>>> Runtime<B> {
    pub fn jit_config(&self) -> JitConfig {
        self.jit.config
    }

    /// Updates automatic compilation bounds, resets heat and applies the live
    /// group limit immediately. Lifetime job accounting is never reset.
    pub fn set_jit_config(&mut self, config: JitConfig) -> Result<()> {
        self.jit.config = config;
        self.set_hot_threshold(
            (self.has_compiler() && self.jit.can_compile()).then_some(config.hot_threshold),
        );
        self.storage.code.as_ref().unwrap().trim(
            &mut self.storage.access,
            usize::from(config.max_groups.get()),
        );
        self.dispatch_table
            .reclaim(&mut self.storage.access, &self.interpret)
    }

    /// Automatic compilation jobs accepted over this runtime's lifetime.
    pub fn jit_compilation_count(&self) -> u32 {
        self.jit.compilations
    }

    /// Takes the pending hot request and attempts to queue an owned capture.
    /// Returns false for stale execution identity, unavailable code, exhausted
    /// allowance, duplicate work or a full queue. No compiler work runs here.
    /// Interpreter cooldown permits progress after a declined request.
    pub fn queue_hot_request(&mut self) -> Result<bool> {
        let Some(request) = self.take_hot_request() else {
            return Ok(false);
        };
        if !self.has_compiler() || !self.jit.can_compile() {
            self.set_hot_threshold(None);
            return Ok(false);
        }
        if !self.compilation.can_queue(request) {
            return Ok(false);
        }
        let cpu = self.machine().cpu().snapshot();
        if request.code_base != cpu.segments.cs.base || request.start_eip != cpu.eip {
            return Ok(false);
        }
        let config = self.jit.config;
        let Some(snapshot) = self.capture_connected_group(
            request.start_eip,
            config.max_blocks,
            config.instructions_per_block,
        ) else {
            return Ok(false);
        };
        if !self.queue_group(snapshot)? {
            return Ok(false);
        }
        self.jit.compilations += 1;
        if !self.jit.can_compile() {
            self.set_hot_threshold(None);
        }
        Ok(true)
    }
}
