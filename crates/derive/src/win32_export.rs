use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{ext::IdentExt, parse::Parser};
use syn::{Attribute, FnArg, Ident, ItemFn, LitStr, Meta, Pat, ReturnType, Signature, Type};

#[derive(Default, PartialEq)]
enum Parameter {
    #[default]
    Argument,
    Context,
    Memory,
    Varargs,
}

fn parameter(attrs: &mut Vec<Attribute>) -> syn::Result<Parameter> {
    let mut role = Parameter::Argument;
    for attr in std::mem::take(attrs) {
        let kind = if attr.path().is_ident("context") {
            Parameter::Context
        } else if attr.path().is_ident("memory") {
            Parameter::Memory
        } else if attr.path().is_ident("varargs") {
            Parameter::Varargs
        } else {
            attrs.push(attr);
            continue;
        };
        if role != Parameter::Argument || !matches!(attr.meta, Meta::Path(_)) {
            return Err(syn::Error::new_spanned(
                attr,
                "expected one parameter binding without arguments",
            ));
        }
        role = kind;
    }
    Ok(role)
}

struct ExportOptions {
    symbol: LitStr,
    abi: Ident,
}

impl ExportOptions {
    fn parse(options: TokenStream, name: &Ident) -> syn::Result<Self> {
        let mut symbol = None;
        let mut abi = None;
        syn::meta::parser(|option| {
            if option.path.is_ident("name") && symbol.is_none() {
                symbol = Some(option.value()?.parse::<LitStr>()?);
            } else if option.path.is_ident("abi") && abi.is_none() {
                let value = option.value()?.parse::<LitStr>()?;
                abi = Some(match value.value().as_str() {
                    "stdcall" => Ident::new("Stdcall", value.span()),
                    "cdecl" => Ident::new("Cdecl", value.span()),
                    _ => return Err(syn::Error::new_spanned(value, "expected stdcall or cdecl")),
                });
            } else {
                return Err(option.error("expected name or abi, each at most once"));
            }
            Ok(())
        })
        .parse2(options)?;
        let abi = abi.unwrap_or_else(|| Ident::new("Stdcall", name.span()));
        let symbol = symbol.unwrap_or_else(|| LitStr::new(&name.unraw().to_string(), name.span()));

        Ok(Self { symbol, abi })
    }
}

struct Parameters {
    fields: Vec<TokenStream>,
    arguments: Vec<TokenStream>,
    context_type: TokenStream,
    context: Ident,
    memory: Ident,
    varargs: Ident,
}

impl Parameters {
    fn parse(signature: &mut Signature, abi: &Ident) -> syn::Result<Self> {
        if signature.constness.is_some()
            || signature.unsafety.is_some()
            || signature.abi.is_some()
            || signature.variadic.is_some()
            || !signature.generics.params.is_empty()
        {
            return Err(syn::Error::new_spanned(
                signature,
                "a DLL export requires a safe, concrete Rust function",
            ));
        }

        let context = Ident::new("__win32_context", Span::mixed_site());
        let memory = Ident::new("__win32_memory", Span::mixed_site());
        let varargs = Ident::new("__win32_varargs", Span::mixed_site());
        let mut context_type = None;
        let mut has_memory = false;
        let mut has_varargs = false;
        let mut arguments = Vec::new();
        let mut fields = Vec::new();
        for argument in &mut signature.inputs {
            let FnArg::Typed(parameter_type) = argument else {
                return Err(syn::Error::new_spanned(
                    argument,
                    "use a free function with an optional #[context] parameter",
                ));
            };
            let Pat::Ident(pattern) = &*parameter_type.pat else {
                return Err(syn::Error::new_spanned(
                    &parameter_type.pat,
                    "expected a named parameter",
                ));
            };
            if pattern.by_ref.is_some() || pattern.subpat.is_some() {
                return Err(syn::Error::new_spanned(
                    pattern,
                    "expected a plain parameter name",
                ));
            }
            let field = &pattern.ident;
            let ty = &parameter_type.ty;
            let value = match parameter(&mut parameter_type.attrs)? {
                Parameter::Argument if !has_varargs => {
                    fields.push(quote!(pub #field: #ty));
                    quote!(self.#field)
                }
                Parameter::Context if context_type.is_none() => {
                    let Type::Reference(reference) = &**ty else {
                        return Err(syn::Error::new_spanned(
                            ty,
                            "context requires a shared reference",
                        ));
                    };
                    if reference.mutability.is_some() {
                        return Err(syn::Error::new_spanned(
                            ty,
                            "context requires a shared reference",
                        ));
                    }
                    context_type = Some(reference.elem.clone());
                    quote!(#context)
                }
                Parameter::Memory if !has_memory => {
                    has_memory = true;
                    quote!(#memory)
                }
                Parameter::Varargs if !has_varargs && abi == "Cdecl" => {
                    has_varargs = true;
                    quote!(#varargs)
                }
                Parameter::Varargs => {
                    return Err(syn::Error::new_spanned(
                        field,
                        "one varargs cursor requires cdecl",
                    ))
                }
                Parameter::Argument => {
                    return Err(syn::Error::new_spanned(
                        field,
                        "fixed parameters precede varargs",
                    ))
                }
                _ => return Err(syn::Error::new_spanned(field, "duplicate service resource")),
            };
            arguments.push(value);
        }

        let context_type = context_type.map_or_else(|| quote!(()), |ty| quote!(#ty));
        Ok(Self {
            fields,
            arguments,
            context_type,
            context,
            memory,
            varargs,
        })
    }
}

pub(super) fn expand(options: TokenStream, mut function: ItemFn) -> syn::Result<TokenStream> {
    let ExportOptions { symbol, abi } = ExportOptions::parse(options, &function.sig.ident)?;
    let Parameters {
        fields,
        arguments,
        context_type,
        context,
        memory,
        varargs,
    } = Parameters::parse(&mut function.sig, &abi)?;
    let name = &function.sig.ident;
    let visible = &function.vis;
    let result = match &function.sig.output {
        ReturnType::Type(_, ty) => quote!(#ty),
        ReturnType::Default => quote!(()),
    };
    let await_result = function.sig.asyncness.map(|_| quote!(.await));
    let cfg: Vec<_> = function
        .attrs
        .iter()
        .filter(|attr| attr.path().is_ident("cfg"))
        .collect();
    let docs = format!("Guest parameters and export binding for `{name}`.");
    Ok(quote! {
        #function

        #(#cfg)*
        #[doc = #docs]
        #[allow(non_camel_case_types)]
        #[derive(::relocade_win32::Win32Args)]
        #visible struct #name { #(#fields,)* }

        #(#cfg)*
        impl ::relocade_win32::Export for #name {
            type Context = #context_type;
            type Output = <#result as ::relocade_win32::ExportResult>::Value;
            const NAME: &'static str = #symbol;
            const SIGNATURE: ::relocade_win32::Signature<Self, Self::Output> =
                ::relocade_win32::Signature::new(::relocade_win32::CallingConvention::#abi);

            async fn invoke(
                self, #context: &Self::Context, #memory: &dyn ::relocade_win32::GuestMemory,
                #varargs: ::relocade_win32::VaList,
            ) -> ::core::result::Result<Self::Output, ::relocade_win32::Exception> {
                ::relocade_win32::ExportResult::into_result(#name(#(#arguments),*)#await_result)
            }
        }
    })
}
