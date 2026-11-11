use crate::{timer_service, Clock, ExecutionWorker, GuestTimeConfig, Runner, TimerHandle};
use crate::{
    Cpu, CpuConfig, CpuStateAccess, CpuStorage, ExecutionInterrupt, IoBus, Memory, Result,
};
use std::future::Future;

/// The single execution capability for a CPU created by [`super::Machine`].
/// Moving it to a worker preserves initialized registers and guest memory.
/// It is deliberately not cloneable: state handles do not grant execution access.
#[derive(crate::worker::Transfer)]
pub struct CpuHandle {
    storage: CpuStorage,
    memory: Memory,
    config: CpuConfig,
    compiler: Option<crate::compiler::Endpoint>,
}

impl CpuHandle {
    pub(super) fn new(
        memory: Memory,
        config: CpuConfig,
        compiler: Option<crate::compiler::Endpoint>,
    ) -> Result<(Self, CpuStateAccess)> {
        memory.configuration().validate_for(config.cpu().mode())?;
        let storage = CpuStorage::new()?;
        let backing = storage.initialize(&config);
        let state = CpuStateAccess::new(&config, backing);
        Ok((
            Self {
                storage,
                memory,
                config,
                compiler,
            },
            state,
        ))
    }

    pub fn memory(&self) -> Memory {
        self.memory.clone()
    }

    pub fn configuration(&self) -> &CpuConfig {
        &self.config
    }

    pub fn interrupt_handle(&self) -> ExecutionInterrupt {
        self.storage.interrupt_handle()
    }

    /// Constructs this CPU's backend and device bindings on its execution owner.
    /// Existing registers, including edits made after creation, are preserved.
    pub async fn into_cpu(self, io: IoBus) -> Result<Cpu> {
        let (cpu, _compiler) = self.into_session(io).await?;
        Ok(cpu)
    }

    pub(crate) async fn into_session(
        self,
        io: IoBus,
    ) -> Result<(Cpu, Option<crate::compiler::Endpoint>)> {
        let cpu = self
            .storage
            .into_initialized_cpu(self.memory, self.config, io)
            .await?;
        Ok((cpu, self.compiler))
    }

    /// Runs this CPU's session with host timers until completion or `stop`.
    /// Await through cleanup: both execution and timer workers are joined before
    /// returning. `None` means the stop signal ended execution.
    pub async fn run_until<T: Send + 'static>(
        self,
        stop: impl Future,
        session: impl AsyncFnOnce(Runner) -> T + Send + 'static,
    ) -> Result<Option<T>> {
        let (handle, timers) = timer_service();
        let execution = self.spawn(handle, session);
        let timers = timers.spawn_with_peer(execution.wake_peer());
        let result = execution.run_until(stop).await;
        timers.stop().await?;
        result
    }

    /// Transfers this CPU to an execution worker using the supplied host timers.
    /// The caller drives the timer service and joins or stops the returned worker.
    pub fn spawn<T: Send + 'static>(
        self,
        host_timers: TimerHandle,
        session: impl AsyncFnOnce(Runner) -> T + Send + 'static,
    ) -> ExecutionWorker<T> {
        self.spawn_with_clock(move || host_timers.into(), session)
    }

    /// Transfers this CPU and constructs its clock on the execution worker.
    pub fn spawn_with_clock<T: Send + 'static>(
        self,
        make_clock: impl FnOnce() -> Clock + Send + 'static,
        session: impl AsyncFnOnce(Runner) -> T + Send + 'static,
    ) -> ExecutionWorker<T> {
        ExecutionWorker::spawn(
            self,
            async move |cpu, endpoint| Runner::new(make_clock(), cpu, endpoint).await,
            session,
        )
    }

    /// Transfers this CPU to a worker using guest time and optional host pacing.
    pub fn spawn_with_guest_time<T: Send + 'static>(
        self,
        make_time: impl FnOnce() -> GuestTimeConfig + Send + 'static,
        session: impl AsyncFnOnce(Runner) -> T + Send + 'static,
    ) -> ExecutionWorker<T> {
        ExecutionWorker::spawn(
            self,
            async move |cpu, endpoint| Runner::with_guest_time(make_time(), cpu, endpoint).await,
            session,
        )
    }
}
