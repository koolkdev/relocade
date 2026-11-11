use relocade_machine::CompilerService;
use std::num::NonZeroUsize;

/// Share one compiler service through the fixture's machine shutdowns.
pub async fn run<T>(test: impl AsyncFnOnce(&CompilerService) -> T) -> T {
    let compiler = CompilerService::start_with_workers(NonZeroUsize::new(1).unwrap()).unwrap();
    let result = test(&compiler).await;
    compiler.shutdown().await.unwrap();
    result
}
