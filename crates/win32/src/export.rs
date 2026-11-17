use crate::{Arguments, Exception, GuestMemory, ReturnValue, Signature, StackReader, VaList};
use std::future::Future;

/// Rust handlers can return a guest value or propagate a checked guest-memory fault.
pub trait ExportResult {
    type Value: ReturnValue;
    fn into_result(self) -> Result<Self::Value, Exception>;
}

impl<R: ReturnValue> ExportResult for R {
    type Value = R;
    fn into_result(self) -> Result<R, Exception> {
        Ok(self)
    }
}

impl<R: ReturnValue> ExportResult for Result<R, Exception> {
    type Value = R;
    fn into_result(self) -> Result<R, Exception> {
        self
    }
}

/// A built-in DLL function declared with [`crate::win32_export`]. The generated
/// parameter record shares the Rust function's name in the type namespace.
pub trait Export: Arguments + 'static {
    type Context: ?Sized + 'static;
    type Output: ReturnValue;
    const NAME: &'static str;
    const SIGNATURE: Signature<Self, Self::Output>;

    #[doc(hidden)]
    fn invoke(
        self,
        context: &Self::Context,
        memory: &dyn GuestMemory,
        varargs: VaList,
    ) -> impl Future<Output = Result<Self::Output, Exception>>;

    /// Enters the service with the original guest call frame. All fixed arguments
    /// are captured before the service can suspend or reenter guest code.
    fn entry<'a>(
        context: &'a Self::Context,
        memory: &'a dyn GuestMemory,
        esp: u32,
    ) -> impl Future<Output = Result<Self::Output, Exception>> + 'a {
        let mut stack = StackReader::at_entry(memory, esp);
        let arguments = Self::SIGNATURE.arguments(&mut stack);
        let varargs = stack.varargs();
        async move { arguments?.invoke(context, memory, varargs).await }
    }
}
