//! Static service selection. Each target owns its ABI declaration.

use proc_macro2::{Ident, Span, TokenStream as Tokens};
use quote::{format_ident, quote};
use syn::{
    braced, parenthesized,
    parse::{Parse, ParseStream},
    punctuated::Punctuated,
    Expr, FnArg, Member, Pat, Token, Visibility,
};

pub(super) struct Table {
    visibility: Visibility,
    name: Ident,
    selector: Ident,
    parameters: Vec<FnArg>,
    entries: Vec<Entry>,
}
struct Entry {
    pattern: Pat,
    target: Expr,
}

impl Parse for Table {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let visibility = input.parse()?;
        let name = input.parse()?;
        let parameters;
        parenthesized!(parameters in input);
        let selector = parameters.parse()?;
        let parameters = if parameters.is_empty() {
            Vec::new()
        } else {
            parameters.parse::<Token![,]>()?;
            Punctuated::<FnArg, Token![,]>::parse_terminated(&parameters)?
                .into_iter()
                .collect()
        };
        let content;
        braced!(content in input);
        let entries = Punctuated::<Entry, Token![,]>::parse_terminated(&content)?
            .into_iter()
            .collect();
        Ok(Self {
            visibility,
            name,
            selector,
            parameters,
            entries,
        })
    }
}
impl Parse for Entry {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let pattern = Pat::parse_multi_with_leading_vert(input)?;
        input.parse::<Token![:]>()?;
        Ok(Self {
            pattern,
            target: input.parse()?,
        })
    }
}
impl Table {
    pub(super) fn expand(self) -> syn::Result<Tokens> {
        let visibility = self.visibility;
        let name = self.name;
        let entry = format_ident!("{name}_entry");
        let selector = self.selector;
        let parameters = self.parameters;
        let call = Ident::new("__interrupt_call", Span::mixed_site());
        let mut arms = Vec::new();
        let mut fallback = false;
        for entry in self.entries {
            let pattern = entry.pattern;
            let is_fallback = matches!(pattern, Pat::Wild(_));
            if fallback {
                return Err(syn::Error::new_spanned(
                    pattern,
                    "fallback must be the last entry",
                ));
            }
            fallback |= is_fallback;
            let invoke = call_entry(entry.target, &call)?;
            arms.push(quote!(#pattern => #invoke.await));
        }
        if !fallback {
            return Err(syn::Error::new_spanned(name, "missing _ fallback"));
        }
        Ok(quote! {
            #visibility async fn #name(#call: &mut ::relocade_machine::CpuContext<'_>, #(#parameters),*) {
                match #call.cpu.#selector() { #(#arms,)* }
            }
            // Nested tables use the same internal adapter lookup as services.
            #[doc(hidden)]
            #[allow(unused_imports)]
            #visibility use self::#name as #entry;
        })
    }
}

pub(super) fn call_entry(target: Expr, call: &Ident) -> syn::Result<Tokens> {
    let (target, args) = match target {
        Expr::Call(call) => (*call.func, call.args),
        target => (target, Punctuated::new()),
    };
    match target {
        Expr::Path(mut path) => {
            let function = &mut path.path.segments.last_mut().unwrap().ident;
            *function = format_ident!("{function}_entry");
            let path = &path.path;
            let qualifier =
                (path.segments.len() == 1 && path.leading_colon.is_none()).then(|| quote!(self::));
            Ok(quote!(#qualifier #path(&mut *#call, #args)))
        }
        Expr::Field(field) => {
            let receiver = field.base;
            let Member::Named(mut name) = field.member else {
                return Err(syn::Error::new_spanned(
                    field.member,
                    "expected a method name",
                ));
            };
            name = format_ident!("{name}_entry");
            Ok(quote!(#receiver.#name(&mut *#call, #args)))
        }
        target => Err(syn::Error::new_spanned(
            target,
            "expected a service function or receiver.method",
        )),
    }
}
