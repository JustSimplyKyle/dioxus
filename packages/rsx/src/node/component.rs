//! Parse components into the VNode::Component variant
//!
//! Uses the regular robust RsxBlock parser and then validates the component, emitting errors as
//! diagnostics. This was refactored from a straightforward parser to this validation approach so
//! that we can emit errors as diagnostics instead of returning results.
//!
//! Using this approach we can provide *much* better errors as well as partial expansion whereever
//! possible.
//!
//! It does lead to the code actually being larger than it was before, but it should be much easier
//! to work with and extend. To add new syntax, we add it to the RsxBlock parser and then add a
//! validation step here. This does make using the component as a source of truth not as good, but
//! oddly enoughly, we want the tree to actually be capable of being technically invalid. This is not
//! usual for building in Rust - you want strongly typed things to be valid - but in this case, we
//! want to accept all sorts of malformed input and then provide the best possible error messages.
//!
//! If you're generally parsing things, you'll just want to parse and then check if it's valid.

use std::collections::HashSet;

use self::location::CallerLocation;

use super::*;

use proc_macro2::TokenStream as TokenStream2;
use proc_macro2_diagnostics::SpanDiagnosticExt;
use quote::quote;
use syn::{
    spanned::Spanned, AngleBracketedGenericArguments, Error, Expr, Ident, LitStr, PathArguments,
    Token,
};

#[derive(PartialEq, Eq, Clone, Debug, Hash)]
pub struct Component {
    pub name: syn::Path,
    pub generics: Option<AngleBracketedGenericArguments>,
    pub fields: Vec<Attribute>,
    pub brace: token::Brace,
    pub children: TemplateBody,
    pub dyn_idx: CallerLocation,
    pub diagnostics: Diagnostics,
}

impl Parse for Component {
    fn parse(stream: ParseStream) -> Result<Self> {
        let RsxBlock {
            name,
            generics,
            fields,
            children,
            brace,
        } = stream.parse::<RsxBlock>()?;

        let mut component = Self {
            diagnostics: Diagnostics::new(),
            dyn_idx: CallerLocation::default(),
            children: TemplateBody::from_nodes(children),
            name,
            generics,
            fields,
            brace,
        };

        component.validate_path();
        component.validate_fields();
        component.validate_key();
        component.validate_spread();

        Ok(component)
    }
}

impl ToTokens for Component {
    fn to_tokens(&self, tokens: &mut TokenStream2) {
        let Self { name, generics, .. } = self;

        // Create props either from manual props or from the builder approach
        let props = self.collect_props();

        // Make sure we stringify the component name
        let fn_name = self.fn_name().to_string();

        // Make sure we emit any errors
        let diagnostics = &self.diagnostics;

        tokens.append_all(quote! {
            dioxus_core::DynamicNode::Component({
                #diagnostics

                use dioxus_core::prelude::Properties;
                (#props).into_vcomponent(
                    #name #generics,
                    #fn_name
                )
            })
        })
    }
}

impl Component {
    fn to_dynamic_node(&self) {}

    fn to_template_node(&self) {}

    // Make sure this a proper component path (uppercase ident, a path, or contains an underscorea)
    // This should be validated by the RsxBlock parser when it peeks bodynodes
    fn validate_path(&mut self) {
        let path = &self.name;

        // First, ensure the path is not a single lowercase ident with no underscores
        if path.segments.len() == 1 {
            let seg = path.segments.first().unwrap();
            if seg.ident.to_string().chars().next().unwrap().is_lowercase()
                && !seg.ident.to_string().contains('_')
            {
                self.diagnostics.push(seg.ident.span().error(
                    "Component names must be uppercase, contain an underscore, or abe a path.",
                ));
            }
        }

        // ensure path segments doesn't have PathArguments, only the last
        // segment is allowed to have one.
        if path
            .segments
            .iter()
            .take(path.segments.len() - 1)
            .any(|seg| seg.arguments != PathArguments::None)
        {
            self.diagnostics.push(path.span().error(
                "Component names must not have path arguments. Only the last segment is allowed to have one.",
            ));
        }

        // ensure last segment only have value of None or AngleBracketed
        if !matches!(
            path.segments.last().unwrap().arguments,
            PathArguments::None | PathArguments::AngleBracketed(_)
        ) {
            self.diagnostics.push(
                path.span()
                    .error("Component names must have no arguments or angle bracketed arguments."),
            );
        }
    }

    // Make sure the spread argument is being used as props spreading
    fn validate_spread(&mut self) {
        // Next, ensure that there's only one spread argument in the attributes *and* it's the last one
        let spread_idx = self
            .fields
            .iter()
            .position(|attr| matches!(attr.value, AttributeValue::Spread(_)));

        if let Some(spread_idx) = spread_idx {
            if spread_idx != self.fields.len() - 1 {
                self.diagnostics.push(
                    self.fields[spread_idx]
                        .name
                        .span()
                        .error("Spread attributes must be the last attribute in the component."),
                );
            }
        }
    }

    /// Ensure only one key and that the key is not a static str
    ///
    /// todo: we want to allow arbitrary exprs for keys provided they impl hash / eq
    fn validate_key(&mut self) {
        let key = self.get_key();

        if let Some(attr) = key {
            let diagnostic = match &attr.value {
                AttributeValue::AttrIfmt(ifmt) if ifmt.is_static() => {
                    ifmt.span().error("Key must not be a static string. Make sure to use a formatted string like `key: \"{value}\"")
                }
                AttributeValue::AttrIfmt(_) => return,
                _ => attr
                    .value
                    .span()
                    .error("Key must be in the form of a formatted string like `key: \"{value}\""),
            };

            self.diagnostics.push(diagnostic);
        }
    }

    pub fn get_key(&self) -> Option<&Attribute> {
        self.fields
            .iter()
            .find(|attr| matches!(&attr.name, AttributeName::Known(key) if key == "key"))
    }

    /// Ensure there's no duplicate props - this will be a compile error but we can move it to a
    /// diagnostic, thankfully
    ///
    /// Also ensure there's no stringly typed propsa
    fn validate_fields(&mut self) {
        let mut seen = HashSet::new();

        for field in self.fields.iter() {
            match &field.name {
                AttributeName::Custom(name) => self.diagnostics.push(
                    name.span()
                        .error("Custom attributes are not supported for Components. Only known attributes are allowed."),
                ),
                AttributeName::Known(k) => {
                    if !seen.contains(k) {
                        seen.insert(k);
                    } else {
                        self.diagnostics.push(
                            k.span()
                                .error("Duplicate attribute found. Only one attribute of each type is allowed."),
                        );
                    }
                },
                AttributeName::Spread(_) => {},
            }
        }
    }

    fn collect_props(&self) -> TokenStream2 {
        let name = &self.name;

        let manual_props = self.manual_props();

        let mut toks = match manual_props.as_ref() {
            Some(props) => quote! { let mut __manual_props = #props; },
            None => match &self.generics {
                Some(gen_args) => quote! { fc_to_builder(#name #gen_args) },
                None => quote! { fc_to_builder(#name) },
            },
        };

        for (name, value) in self.make_field_idents() {
            match manual_props.is_none() {
                true => toks.append_all(quote! { .#name(#value) }),
                false => toks.append_all(quote! { __manual_props.#name = #value; }),
            }
        }

        if !self.children.is_empty() {
            let children = &self.children;
            match manual_props.is_none() {
                true => toks.append_all(quote! { .children( { #children } ) }),
                false => toks.append_all(quote! { __manual_props.children = { #children }; }),
            }
        }

        match manual_props.is_none() {
            true => toks.append_all(quote! { .build() }),
            false => toks.append_all(quote! { __manual_props }),
        }

        toks
    }

    fn manual_props(&self) -> Option<&Expr> {
        self.fields.iter().find_map(|attr| match attr.value {
            AttributeValue::Spread(ref expr) => Some(expr),
            _ => None,
        })
    }

    fn make_field_idents(&self) -> Vec<(TokenStream2, TokenStream2)> {
        self.fields
            .iter()
            .filter_map(|attr| {
                let Attribute { name, value, .. } = attr;

                let attr = match name {
                    AttributeName::Known(k) => {
                        if k.to_string() == "key" {
                            return None;
                        }
                        quote! { #k }
                    }
                    AttributeName::Custom(_) => return None,
                    AttributeName::Spread(_) => return None,
                };

                let val = match value {
                    AttributeValue::Spread(_) => return None,
                    AttributeValue::AttrIfmt(ifmt) => {
                        quote! {
                            #ifmt.to_string()
                        }
                    }
                    _ => value.to_token_stream(),
                };

                Some((attr, val))
            })
            .collect()
    }

    fn fn_name(&self) -> Ident {
        self.name.segments.last().unwrap().ident.clone()
    }

    // pub fn key(&self) -> Option<&IfmtInput> {
    //     self.key.as_ref()
    // }
}

mod tests {
    use super::*;

    /// Ensure we can parse a component
    #[test]
    fn parses() {
        let input = quote! {
            MyComponent {
                key: "value {something}",
                prop: "value",
                ..props,
                div {
                    "Hello, world!"
                }
            }
        };

        let component: Component = syn::parse2(input).unwrap();

        dbg!(component);

        let input_without_manual_props = quote! {
            MyComponent {
                key: "value {something}",
                prop: "value",
                div { "Hello, world!" }
            }
        };

        let component: Component = syn::parse2(input_without_manual_props).unwrap();
        dbg!(component);
    }

    /// Ensure we reject invalid forms
    ///
    /// Maybe want to snapshot the errors?
    #[test]
    fn rejects() {
        let input = quote! {
            myComponent {
                key: "value",
                prop: "value",
                prop: "other",
                ..props,
                ..other_props,
                div {
                    "Hello, world!"
                }
            }
        };

        let mut component: Component = syn::parse2(input).unwrap();
        dbg!(component.diagnostics);
    }

    #[test]
    fn to_tokens_properly() {
        let input = quote! {
            MyComponent {
                key: "value {something}",
                prop: "value",
                ..props,
                div {
                    "Hello, world!"
                }
            }
        };

        let component: Component = syn::parse2(input).unwrap();

        let mut tokens = TokenStream2::new();
        component.to_tokens(&mut tokens);

        dbg!(tokens.to_string());

        // let input_without_manual_props = quote! {
        //     MyComponent {
        //         key: "value {something}",
        //         prop: "value",
        //         div { "Hello, world!" }
        //     }
        // };

        // let component: Component = syn::parse2(input_without_manual_props).unwrap();

        // let mut tokens = TokenStream2::new();
        // component.to_tokens(&mut tokens);

        // dbg!(tokens.to_string());
    }

    #[test]
    fn as_template_node() {}
}
