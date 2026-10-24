#![allow(dead_code)]
use relocade_machine::{CpuConfig, MachineConfig};

/// Initial state for a test's machine and its first CPU.
pub struct Configuration {
    pub cpu: CpuConfig,
    pub machine: MachineConfig,
}

pub fn configuration(
    target: relocade_x86::ExecutionTarget,
    cpu: relocade_x86::CpuState,
    size: u64,
    map: relocade_x86::PhysicalMemoryMap,
) -> Result<Configuration, relocade_machine::ConfigurationError> {
    Ok(Configuration {
        cpu: CpuConfig::new(target, cpu)?,
        machine: MachineConfig::new(size, map)?,
    })
}

pub async fn create_cpu<T: 'static>(
    state: T,
    Configuration {
        cpu,
        machine: config,
    }: Configuration,
) -> relocade_machine::Result<relocade_machine::Cpu<T>> {
    relocade_machine::Cpu::with_state(config, cpu, state).await
}

/// Poll a run once so tests can observe suspension without keeping its waiter.
pub fn poll_once<T>(future: impl std::future::Future<Output = T>) -> std::task::Poll<T> {
    let mut future = std::pin::pin!(future);
    future
        .as_mut()
        .poll(&mut std::task::Context::from_waker(std::task::Waker::noop()))
}

/// Ready-input fixtures must finish without an executor turn or a blocking wait.
pub fn ready<T>(future: impl std::future::Future<Output = T>) -> T {
    match poll_once(future) {
        std::task::Poll::Ready(value) => value,
        std::task::Poll::Pending => panic!("expected the run to complete in one poll"),
    }
}
