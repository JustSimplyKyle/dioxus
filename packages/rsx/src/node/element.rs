use std::fmt::{Display, Formatter};

use super::*;

use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::quote;
use syn::{
    parse::ParseBuffer, punctuated::Punctuated, spanned::Spanned, token::Brace, Expr, Ident,
    LitStr, Token,
};

/// Parse the VNode::Element type
#[derive(PartialEq, Eq, Clone, Debug, Hash)]
pub struct Element {
    pub name: ElementName,
    pub key: Option<IfmtInput>,
    pub attributes: Vec<AttributeType>,
    pub merged_attributes: Vec<AttributeType>,
    pub brace: syn::token::Brace,
    pub children: Vec<BodyNode>,
}

impl Parse for Element {
    fn parse(stream: ParseStream) -> Result<Self> {
        let el_name = ElementName::parse(stream)?;

        // parse the guts
        let content: ParseBuffer;
        let brace = syn::braced!(content in stream);

        let mut attributes: Vec<AttributeType> = vec![];
        let mut children: Vec<BodyNode> = vec![];
        let mut key = None;

        // parse fields with commas
        // break when we don't get this pattern anymore
        // start parsing bodynodes
        // "def": 456,
        // abc: 123,
        loop {
            if content.peek(Token![..]) {
                content.parse::<Token![..]>()?;
                let expr = content.parse::<Expr>()?;
                let span = expr.span();
                attributes.push(attribute::AttributeType::Spread(expr));

                if content.is_empty() {
                    break;
                }

                if content.parse::<Token![,]>().is_err() {
                    missing_trailing_comma!(span);
                }
                continue;
            }

            // Parse the raw literal fields
            // "def": 456,
            if content.peek(LitStr) && content.peek2(Token![:]) && !content.peek3(Token![:]) {
                let name = content.parse::<LitStr>()?;
                let ident = name.clone();

                content.parse::<Token![:]>()?;

                let value = content.parse::<ElementAttrValue>()?;
                attributes.push(attribute::AttributeType::Named(ElementAttrNamed {
                    el_name: el_name.clone(),
                    attr: ElementAttr {
                        name: ElementAttrName::Custom(name),
                        value,
                    },
                }));

                if content.is_empty() {
                    break;
                }

                if content.parse::<Token![,]>().is_err() {
                    missing_trailing_comma!(ident.span());
                }
                continue;
            }

            // Parse
            // abc: 123,
            if content.peek(Ident) && content.peek2(Token![:]) && !content.peek3(Token![:]) {
                let name = content.parse::<Ident>()?;

                let name_str = name.to_string();
                content.parse::<Token![:]>()?;

                // The span of the content to be parsed,
                // for example the `hi` part of `class: "hi"`.
                let span = content.span();

                if name_str.starts_with("on") {
                    // check for any duplicate event listeners
                    if attributes.iter().any(|f| {
                        if let AttributeType::Named(ElementAttrNamed {
                            attr:
                                ElementAttr {
                                    name: ElementAttrName::BuiltIn(n),
                                    value: ElementAttrValue::EventTokens(_),
                                },
                            ..
                        }) = f
                        {
                            n == &name_str
                        } else {
                            false
                        }
                    }) {
                        return Err(syn::Error::new(
                            name.span(),
                            format!("Duplicate event listener `{}`", name),
                        ));
                    }
                    attributes.push(attribute::AttributeType::Named(ElementAttrNamed {
                        el_name: el_name.clone(),
                        attr: ElementAttr {
                            name: ElementAttrName::BuiltIn(name),
                            value: ElementAttrValue::EventTokens(content.parse()?),
                        },
                    }));
                } else if name_str == "key" {
                    let _key: IfmtInput = content.parse()?;

                    if _key.is_static() {
                        invalid_key!(_key);
                    }

                    key = Some(_key);
                } else {
                    let value = content.parse::<ElementAttrValue>()?;
                    attributes.push(attribute::AttributeType::Named(ElementAttrNamed {
                        el_name: el_name.clone(),
                        attr: ElementAttr {
                            name: ElementAttrName::BuiltIn(name),
                            value,
                        },
                    }));
                }

                if content.is_empty() {
                    break;
                }

                if content.parse::<Token![,]>().is_err() {
                    missing_trailing_comma!(span);
                }
                continue;
            }

            // Parse shorthand fields
            if content.peek(Ident)
                && !content.peek2(Brace)
                && !content.peek2(Token![:])
                && !content.peek2(Token![-])
            {
                let name = content.parse::<Ident>()?;
                let name_ = name.clone();

                // If the shorthand field is children, these are actually children!
                if name == "children" {
                    return Err(syn::Error::new(
                        name.span(),
                        r#"Shorthand element children are not supported.
To pass children into elements, wrap them in curly braces.
Like so:
    div {{ {{children}} }}

"#,
                    ));
                };

                let value = ElementAttrValue::Shorthand(name.clone());
                attributes.push(attribute::AttributeType::Named(ElementAttrNamed {
                    el_name: el_name.clone(),
                    attr: ElementAttr {
                        name: ElementAttrName::BuiltIn(name),
                        value,
                    },
                }));

                if content.is_empty() {
                    break;
                }

                if content.parse::<Token![,]>().is_err() {
                    missing_trailing_comma!(name_.span());
                }
                continue;
            }

            break;
        }

        while !content.is_empty() {
            if (content.peek(LitStr) && content.peek2(Token![:])) && !content.peek3(Token![:]) {
                attr_after_element!(content.span());
            }

            if (content.peek(Ident) && content.peek2(Token![:])) && !content.peek3(Token![:]) {
                attr_after_element!(content.span());
            }

            children.push(content.parse::<BodyNode>()?);
            // consume comma if it exists
            // we don't actually care if there *are* commas after elements/text
            if content.peek(Token![,]) {
                let _ = content.parse::<Token![,]>();
            }
        }

        // Now merge the attributes into the cache
        let mut merged_attributes: Vec<AttributeType> = Vec::new();
        for attr in &attributes {
            let attr_index = merged_attributes
                .iter()
                .position(|a| a.matches_attr_name(attr));

            if let Some(old_attr_index) = attr_index {
                let old_attr = &mut merged_attributes[old_attr_index];

                if let Some(combined) = old_attr.try_combine(attr) {
                    *old_attr = combined;
                }

                continue;
            }

            merged_attributes.push(attr.clone());
        }

        Ok(Element {
            name: el_name,
            key,
            attributes,
            merged_attributes,
            children,
            brace,
        })
    }
}

impl ToTokens for Element {
    fn to_tokens(&self, tokens: &mut TokenStream2) {
        let el = self;

        let el_name = &el.name;
        let ns = |name| match el_name {
            ElementName::Ident(i) => quote! { dioxus_elements::#i::#name },
            ElementName::Custom(_) => quote! { None },
        };

        let static_attrs = el
            .merged_attributes
            .iter()
            .map(|attr| {
                // Rendering static attributes requires a bit more work than just a dynamic attrs
                match attr.as_static_str_literal() {
                    // If it's static, we'll take this little optimization
                    Some((name, value)) => {
                        let value = value.to_static().unwrap();

                        let ns = match name {
                            ElementAttrName::BuiltIn(name) => ns(quote!(#name.1)),
                            ElementAttrName::Custom(_) => quote!(None),
                        };

                        let name = match (el_name, name) {
                            (ElementName::Ident(_), ElementAttrName::BuiltIn(_)) => {
                                quote! { #el_name::#name.0 }
                            }
                            _ => {
                                //hmmmm I think we could just totokens this, but the to_string might be inserting quotes
                                let as_string = name.to_string();
                                quote! { #as_string }
                            }
                        };

                        quote! {
                            dioxus_core::TemplateAttribute::Static {
                                name: #name,
                                namespace: #ns,
                                value: #value,

                                // todo: we don't diff these so we never apply the volatile flag
                                // volatile: dioxus_elements::#el_name::#name.2,
                            },
                        }
                    }

                    // Otherwise, we'll just render it as a dynamic attribute
                    // This will also insert the attribute into the dynamic_attributes list to assemble the final template
                    _ => {
                        //
                        todo!()
                    }
                }
            })
            .collect::<Vec<_>>();

        // Render either the static child or the dynamic child
        let children = el.children.iter().map(|c| match c {
            BodyNode::Element(el) => quote! { #el },
            BodyNode::Text(text) if text.is_static() => {
                let text = text.input.to_static().unwrap();
                quote! { dioxus_core::TemplateNode::Text { text: #text } }
            }
            BodyNode::Text(text) => {
                let id = text.dyn_idx.get();
                quote! { dioxus_core::TemplateNode::DynamicText { id: #id } }
            }
            BodyNode::ForLoop(floop) => {
                let id = floop.dyn_idx.get();
                quote! { dioxus_core::TemplateNode::Dynamic { id: #id } }
            }
            BodyNode::RawExpr(exp) => {
                let id = exp.dyn_idx.get();
                quote! { dioxus_core::TemplateNode::Dynamic { id: #id } }
            }
            BodyNode::Component(exp) => {
                let id = exp.dyn_idx.get();
                quote! { dioxus_core::TemplateNode::Dynamic { id: #id } }
            }
            BodyNode::IfChain(exp) => {
                let id = exp.dyn_idx.get();
                quote! { dioxus_core::TemplateNode::Dynamic { id: #id } }
            }
        });

        let ns = ns(quote!(NAME_SPACE));
        let el_name = el_name.tag_name();

        tokens.append_all(quote! {
            dioxus_core::TemplateNode::Element {
                tag: #el_name,
                namespace: #ns,
                attrs: &[ #(#static_attrs)* ],
                children: &[ #(#children),* ],
            }
        })
    }
}

#[derive(PartialEq, Eq, Clone, Debug, Hash)]
pub enum ElementName {
    Ident(Ident),
    Custom(LitStr),
}

impl ElementName {
    pub(crate) fn tag_name(&self) -> TokenStream2 {
        match self {
            ElementName::Ident(i) => quote! { dioxus_elements::#i::TAG_NAME },
            ElementName::Custom(s) => quote! { #s },
        }
    }
}

impl ElementName {
    pub fn span(&self) -> Span {
        match self {
            ElementName::Ident(i) => i.span(),
            ElementName::Custom(s) => s.span(),
        }
    }
}

impl PartialEq<&str> for ElementName {
    fn eq(&self, other: &&str) -> bool {
        match self {
            ElementName::Ident(i) => i == *other,
            ElementName::Custom(s) => s.value() == *other,
        }
    }
}

impl Display for ElementName {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ElementName::Ident(i) => write!(f, "{}", i),
            ElementName::Custom(s) => write!(f, "{}", s.value()),
        }
    }
}

impl Parse for ElementName {
    fn parse(stream: ParseStream) -> Result<Self> {
        let raw = Punctuated::<Ident, Token![-]>::parse_separated_nonempty(stream)?;
        if raw.len() == 1 {
            Ok(ElementName::Ident(raw.into_iter().next().unwrap()))
        } else {
            let span = raw.span();
            let tag = raw
                .into_iter()
                .map(|ident| ident.to_string())
                .collect::<Vec<_>>()
                .join("-");
            let tag = LitStr::new(&tag, span);
            Ok(ElementName::Custom(tag))
        }
    }
}

impl ToTokens for ElementName {
    fn to_tokens(&self, tokens: &mut TokenStream2) {
        match self {
            ElementName::Ident(i) => tokens.append_all(quote! { dioxus_elements::#i }),
            ElementName::Custom(s) => tokens.append_all(quote! { #s }),
        }
    }
}
