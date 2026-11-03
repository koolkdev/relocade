//! Refer to or invoke a service through its generated register adapter.

use proc_macro2::{Ident, Span, TokenStream as Tokens};
use quote::{format_ident, quote};
use syn::{punctuated::Punctuated, Expr, ExprPath, Member};

pub(crate) fn handler(target: Expr) -> syn::Result<Tokens> {
    if let Expr::Path(path) = target {
        return Ok(adapter_path(path));
    }
    let call = Ident::new("call", Span::mixed_site());
    let invoke = entry(target, &call)?;
    Ok(quote! {
        async move |#call: &mut ::relocade_machine::CpuContext<'_>| {
            #invoke.await
        }
    })
}

pub(crate) fn entry(target: Expr, call: &Ident) -> syn::Result<Tokens> {
    let (target, args) = match target {
        Expr::Call(call) => (*call.func, call.args),
        Expr::MethodCall(mut method) => {
            method.method = format_ident!("{}_entry", method.method);
            method.args.insert(0, syn::parse_quote!(&mut *#call));
            return Ok(quote!(#method));
        }
        target => (target, Punctuated::new()),
    };
    match target {
        Expr::Path(path) => {
            let path = adapter_path(path);
            Ok(quote!(#path(&mut *#call, #args)))
        }
        Expr::Field(field) => {
            let receiver = field.base;
            let Member::Named(name) = field.member else {
                return Err(syn::Error::new_spanned(
                    field.member,
                    "expected a method name",
                ));
            };
            let name = format_ident!("{name}_entry");
            Ok(quote!(#receiver.#name(&mut *#call, #args)))
        }
        target => Err(syn::Error::new_spanned(
            target,
            "expected a service function or receiver.method",
        )),
    }
}

fn adapter_path(mut target: ExprPath) -> Tokens {
    let function = &mut target.path.segments.last_mut().unwrap().ident;
    *function = format_ident!("{function}_entry");
    let qualifier = (target.qself.is_none()
        && target.path.segments.len() == 1
        && target.path.leading_colon.is_none())
    .then(|| quote!(self::));
    quote!(#qualifier #target)
}
