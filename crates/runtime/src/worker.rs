//! Thread tasks and transport for native values and browser objects.

#[cfg(all(target_arch = "wasm32", not(target_feature = "atomics")))]
compile_error!("worker requires a threaded browser build; see runtime/docs/workers.md");

pub use web_thread::Thread;

/// Carries a payload through the thread library's existing `Post` transport.
///
/// Native payloads move as Send Rust values. Browser payloads use Serde and
/// structured cloning; backend resource types preserve their JavaScript handles.
/// Shared memories keep the same backing bytes. This wrapper adds no transfer
/// list for objects that require one and provides no memory synchronization.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Message<T>(pub T);

#[cfg(target_arch = "wasm32")]
impl<T: serde::Serialize + serde::de::DeserializeOwned> web_thread::Post for Message<T> {}
