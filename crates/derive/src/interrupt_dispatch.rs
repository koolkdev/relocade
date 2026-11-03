//! Select ABI adapters using an ordinary Rust match expression.

use proc_macro2::{Ident, TokenStream as Tokens};
use quote::quote;
use syn::{
    ext::IdentExt,
    parse::{Parse, ParseStream},
    visit::{self, Visit},
    BinOp, Expr, ExprMatch, PatIdent, Token,
};

pub(super) struct Selection {
    call: Ident,
    expression: ExprMatch,
}

impl Parse for Selection {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let call = input.parse()?;
        input.parse::<Token![,]>()?;
        let expression = input.parse()?;
        Ok(Self { call, expression })
    }
}

impl Selection {
    pub(super) fn expand(self) -> syn::Result<Tokens> {
        let mut expression = self.expression;
        for arm in &mut expression.arms {
            let mut binding = ContextBinding {
                name: &self.call,
                shadow: None,
            };
            binding.visit_pat(&arm.pat);
            if let Some((_, guard)) = &arm.guard {
                binding.visit_expr(guard);
            }
            if let Some(name) = binding.shadow {
                return Err(syn::Error::new_spanned(
                    name,
                    "match bindings cannot shadow the interrupt context",
                ));
            }
            let invoke = crate::interrupt::call::entry(*arm.body.clone(), &self.call)?;
            arm.body = syn::parse_quote!(#invoke.await);
        }
        Ok(quote!(#expression))
    }
}

struct ContextBinding<'a> {
    name: &'a Ident,
    shadow: Option<&'a Ident>,
}

impl<'a> Visit<'a> for ContextBinding<'a> {
    fn visit_pat_ident(&mut self, pattern: &'a PatIdent) {
        if pattern.ident.unraw() == self.name.unraw() {
            self.shadow = Some(&pattern.ident);
        }
        visit::visit_pat_ident(self, pattern);
    }

    fn visit_expr(&mut self, expression: &'a Expr) {
        // Only top-level let guards introduce bindings into the arm body.
        match expression {
            Expr::Let(expression) => self.visit_pat(&expression.pat),
            Expr::Binary(expression) if matches!(expression.op, BinOp::And(_)) => {
                self.visit_expr(&expression.left);
                self.visit_expr(&expression.right);
            }
            Expr::Paren(expression) => self.visit_expr(&expression.expr),
            Expr::Group(expression) => self.visit_expr(&expression.expr),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Selection;
    use quote::quote;

    #[test]
    fn pattern_bindings_cannot_replace_the_interrupt_context() {
        for context in [quote!(call), quote!(r#call)] {
            for pattern in [quote!(call), quote!(Some(r#call)), quote!(Record { call })] {
                let selection = syn::parse2::<Selection>(quote! {
                    #context, match value { #pattern => service }
                })
                .unwrap();
                assert_eq!(
                    selection.expand().unwrap_err().to_string(),
                    "match bindings cannot shadow the interrupt context"
                );
            }
        }
    }

    #[test]
    fn guard_bindings_cannot_replace_the_context_but_local_bindings_can() {
        let selection = syn::parse2::<Selection>(quote! {
            call, match value {
                _ if ready && let Some(r#call) = other => service,
                _ => fallback,
            }
        })
        .unwrap();
        assert_eq!(
            selection.expand().unwrap_err().to_string(),
            "match bindings cannot shadow the interrupt context"
        );
        let selection = syn::parse2::<Selection>(quote! {
            call, match value {
                _ if { let call = true; call } => service,
                _ => fallback,
            }
        })
        .unwrap();
        selection.expand().unwrap();
    }
}
