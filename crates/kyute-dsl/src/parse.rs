use crate::{ColorSpec, Element, PropertyBinding, PropertyExpr, SourceLocation, Template};
use proc_macro2::{Literal, Span, TokenStream};
use quote::{ToTokens, TokenStreamExt};
use syn::parse::{Parse, ParseStream};
use syn::{Attribute, Ident, Token, TypePath};

/*//--------------------------------------------------------------------------------------------------
struct CrateName;

const C: CrateName = CrateName;

impl ToTokens for CrateName {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.append(syn::Ident::new("kyute", Span::call_site()))
    }
}*/

//--------------------------------------------------------------------------------------------------

/// Custom keywords.
mod kw {
    //syn::custom_keyword!(property);
    //syn::custom_keyword!(event);
    //syn::custom_keyword!(control);
}

//--------------------------------------------------------------------------------------------------

impl From<Span> for SourceLocation {
    fn from(span: Span) -> Self {
        let start = span.start();
        let end = span.end();
        SourceLocation {
            file: "unknown".to_string(),    // TODO
            start_line: start.line as u32,
            start_col: start.column as u32,
            end_line: end.line as u32,
            end_col: end.column as u32,
        }
    }
}

impl Parse for PropertyExpr {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        if input.peek(syn::LitStr) {
            Ok(PropertyExpr::String(input.parse::<syn::LitStr>()?.value()))
        } else if input.peek(syn::LitInt) {
            let lit: syn::LitInt = input.parse()?;
            match lit.suffix() {
                "px" => return Ok(PropertyExpr::Px(lit.base10_parse()?)),
                "fr" => return Ok(PropertyExpr::Fr(lit.base10_parse()?)),
                _ => Ok(PropertyExpr::Int(lit.base10_parse()?))
            }
        } else if input.peek(Token![#]) {
            let _: Token![#] = input.parse()?;
            let lit: Literal = input.parse()?;
            Ok(PropertyExpr::Color(ColorSpec::Hex(lit.to_string())))
        } else {
            Err(input.error("expected string, integer, or color literal"))
        }
    }
}


/// Items within a UI template.
#[derive(Debug)]
pub enum ElementItem {
    PropertyInit(PropertyBinding),
    /// Child element declaration.
    Element(Element),
}

impl Parse for ElementItem {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        // attributes
        let attrs = input.call(Attribute::parse_outer)?;
        let lookahead = input.lookahead1();
        if lookahead.peek(Token![<]) {
            Element::parse_rest(attrs, None, input).map(ElementItem::Element)
        } else {
            let name: Ident = input.parse()?;

            let lookahead = input.lookahead1();
            if lookahead.peek(Token![:]) {
                // property initialization
                PropertyBinding::parse_rest(attrs, name, input).map(ElementItem::PropertyInit)
            } else if lookahead.peek(Token![=]) {
                // named element
                let _eq: Token![=] = input.parse()?;
                Element::parse_rest(attrs, Some(name), input).map(ElementItem::Element)
            } else {
                // syntax error
                Err(lookahead.error())
            }
        }
    }
}

impl Element {
    /// Parse the rest of an element declaration, after attributes and the optional name.
    fn parse_rest(_attrs: Vec<Attribute>, name: Option<Ident>, input: ParseStream) -> syn::Result<Self> {
        let _: Token![<] = input.parse()?;
        let path = input.parse::<TypePath>()?;
        let _: Token![>] = input.parse()?;

        let content;
        let _braces = syn::braced!(content in input);

        let mut children: Vec<Element> = Vec::new();
        let mut properties: Vec<PropertyBinding> = Vec::new();
        while !content.is_empty() {
            match content.parse()? {
                ElementItem::Element(elem) => {
                    children.push(elem);
                }
                ElementItem::PropertyInit(prop) => {
                    properties.push(prop);
                }
            }
        }

        Ok(Element {
            name: name.map(|n| n.to_string()),
            children,
            properties,
            ty: path.to_token_stream().to_string(),
        })
    }
}


impl PropertyBinding {
    fn parse_rest(_attrs: Vec<Attribute>, name: Ident, input: ParseStream) -> syn::Result<Self> {
        let _: Token![:] = input.parse()?;
        let expr: PropertyExpr = input.parse()?;
        let _semicolon: Token![;] = input.parse()?;
        Ok(PropertyBinding { name: name.to_string(), expr })
    }
}

impl Parse for Template {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let start = input.span();
        let item: ElementItem = input.parse()?;
        let end = input.span();
        let span = start.join(end).unwrap_or(start);

        let root = match item {
            ElementItem::Element(elem) => elem,
            ElementItem::PropertyInit(_) => {
                return Err(syn::Error::new(span, "expected element declaration"))
            }
        };

        Ok(Template { root, location: span.into() })
    }
}