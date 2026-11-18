use crate::{Export, Exports};
use std::rc::Rc;

/// A PE import or export lookup key. Ordinals are the values published by the DLL.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum DllSymbol<'a> {
    Name(&'a str),
    Ordinal(u16),
}

/// The file name and public function table of a built-in PE32 module.
/// A definition can be shared by processes that load it at different addresses.
///
/// ```
/// use relocade_machine::Cpu;
/// use relocade_win32::{win32_export, BuiltinDll, DllExport, DllLoader, DllSymbol};
/// use std::rc::Rc;
///
/// struct Process { id: u32 }
///
/// #[win32_export(name = "GetCurrentProcessId")]
/// fn process_id(#[context] process: &Process) -> u32 { process.id }
///
/// const KERNEL32: BuiltinDll<Process> = BuiltinDll::new(
///     "kernel32.dll",
///     &[DllExport::function::<process_id>()],
/// );
///
/// async fn bind_process_id(loader: &mut DllLoader, cpu: &mut Cpu, process: Rc<Process>,
///                          import_slot: u32) -> Result<(), Box<dyn std::error::Error>> {
///     loader.load(cpu, &KERNEL32, process).await?;
///     loader.bind_import("KERNEL32.DLL", DllSymbol::Name("GetCurrentProcessId"), import_slot)?
///         .ok_or("missing GetCurrentProcessId")?;
///     Ok(())
/// }
/// ```
pub struct BuiltinDll<C: ?Sized + 'static> {
    pub(crate) name: &'static str,
    pub(crate) exports: &'static [DllExport<C>],
}

impl<C: ?Sized + 'static> BuiltinDll<C> {
    pub const fn new(name: &'static str, exports: &'static [DllExport<C>]) -> Self {
        Self { name, exports }
    }
}

/// One function with a single guest address shared by all its public names and ordinal.
pub struct DllExport<C: ?Sized> {
    name: Option<&'static str>,
    aliases: &'static [&'static str],
    ordinal: Option<u16>,
    pub(crate) stub: &'static [u8],
    pub(crate) hook: fn(&mut Exports<'_>, u32, Rc<C>),
}

impl<C: ?Sized + 'static> DllExport<C> {
    /// Uses the export's declared symbol, ABI, and process-supplied context.
    pub const fn function<E: Export<Context = C>>() -> Self {
        Self::new::<E>(|exports, address, context| exports.hook::<E>(address, context))
    }

    /// Includes a context-free function in a DLL alongside stateful exports.
    pub const fn stateless<E: Export<Context = ()>>() -> Self {
        Self::new::<E>(|exports, address, _| exports.hook::<E>(address, ()))
    }

    const fn new<E: Export>(hook: fn(&mut Exports<'_>, u32, Rc<C>)) -> Self {
        Self {
            name: Some(E::NAME),
            aliases: &[],
            ordinal: None,
            stub: E::SIGNATURE.stub_bytes(),
            hook,
        }
    }

    /// Publishes additional case-sensitive names for this function.
    pub const fn aliases(mut self, aliases: &'static [&'static str]) -> Self {
        assert!(
            self.name.is_some(),
            "ordinal-only exports cannot have aliases"
        );
        self.aliases = aliases;
        self
    }

    /// Publishes an explicit PE ordinal alongside the function's names.
    pub const fn ordinal(mut self, ordinal: u16) -> Self {
        self.ordinal = Some(ordinal);
        self
    }

    /// Exposes the function by ordinal alone, omitting its declared name and aliases.
    pub const fn ordinal_only(mut self, ordinal: u16) -> Self {
        self.name = None;
        self.aliases = &[];
        self.ordinal(ordinal)
    }

    pub(crate) fn symbols(&self) -> impl Iterator<Item = DllSymbol<'static>> + '_ {
        self.name
            .into_iter()
            .chain(self.aliases.iter().copied())
            .map(DllSymbol::Name)
            .chain(self.ordinal.map(DllSymbol::Ordinal))
    }
}
