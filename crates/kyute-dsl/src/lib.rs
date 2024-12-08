use serde::{Deserialize, Serialize};

mod parse;

pub use proc_macro2::Span;
use kyute_common::Color;

/// Source location.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceLocation {
    /// File path.
    pub file: String,
    /// Start line.
    pub start_line: u32,
    /// Start column.
    pub start_col: u32,
    /// End line.
    pub end_line: u32,
    /// End column.
    pub end_col: u32,
}

/// Color specification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ColorSpec {
    /// HEX color.
    Hex(String),
    /// U8 RGB color (from rgb() expression).
    Rgb(u8, u8, u8),
    /// U8 RGBA color (from rgba() expression).
    Rgba(u8, u8, u8, u8),
}

/// Property binding expression.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PropertyExpr {
    /// Px size literal, like `34px`.
    Px(f32),
    /// Fractional size literal, like `1fr`.
    Fr(f32),
    /// Integer literal without a suffix.
    Int(i64),
    /// Floating-point literal.
    Float(f64),
    /// String literal.
    String(String),
    /// Color literal.
    Color(ColorSpec),
}

impl PropertyExpr {
    pub fn cast<'a, T: TryFrom<&'a Self> + 'a>(&'a self) -> Result<T, T::Error> {
        T::try_from(self)
    }

    pub fn cast_or_default<'a, T: TryFrom<&'a Self> + Default + 'a>(&'a self) -> T {
        T::try_from(self).unwrap_or_default()
    }
}

impl<'a> TryFrom<&'a PropertyExpr> for Color {
    type Error = &'static str;

    fn try_from(value: &'a PropertyExpr) -> Result<Self, Self::Error> {
        match value {
            PropertyExpr::Color(ColorSpec::Hex(hex)) => {
                Ok(Color::try_from_hex(&hex).map_err(|_| "invalid hex color string")?)
            }
            PropertyExpr::Color(ColorSpec::Rgb(r, g, b)) => Ok(Color::from_rgb_u8(*r, *g, *b)),
            PropertyExpr::Color(ColorSpec::Rgba(r, g, b, a)) => Ok(Color::from_rgba_u8(*r, *g, *b, *a)),
            _ => Err("expected color expression"),
        }
    }
}

impl<'a> TryFrom<&'a PropertyExpr> for f32 {
    type Error = &'static str;

    fn try_from(value: &'a PropertyExpr) -> Result<Self, Self::Error> {
        match value {
            PropertyExpr::Int(i) => Ok(*i as f32),
            PropertyExpr::Float(f) => Ok(*f as f32),
            _ => Err("expected integer expression"),
        }
    }
}

impl<'a> TryFrom<&'a PropertyExpr> for f64 {
    type Error = &'static str;

    fn try_from(value: &'a PropertyExpr) -> Result<Self, Self::Error> {
        match value {
            PropertyExpr::Int(i) => Ok(*i as f64),
            PropertyExpr::Float(f) => Ok(*f),
            _ => Err("expected integer expression"),
        }
    }
}

impl<'a> TryFrom<&'a PropertyExpr> for String {
    type Error = &'static str;

    fn try_from(value: &'a PropertyExpr) -> Result<Self, Self::Error> {
        match value {
            PropertyExpr::String(s) => Ok(s.clone()),
            _ => Err("expected string expression"),
        }
    }
}

impl<'a> TryFrom<&'a PropertyExpr> for i64 {
    type Error = &'static str;

    fn try_from(value: &'a PropertyExpr) -> Result<Self, Self::Error> {
        match value {
            PropertyExpr::Int(i) => Ok(*i),
            _ => Err("expected integer expression"),
        }
    }
}

/// A property binding within a component declaration, like `text: "hello world"`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyBinding {
    /// Name of the property.
    pub name: String,
    /// Value of the property.
    pub expr: PropertyExpr,
}

/// An element in the UI tree, like `<Frame> { ... }` or `name = <Frame> { ... }`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Element {
    /// Name of the element, if it has one.
    pub name: Option<String>,
    /// Type of the element to be instantiated.
    pub ty: String,
    /// Properties to initialize on the element.
    pub properties: Vec<PropertyBinding>,
    /// Child element indices.
    pub children: Vec<Element>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Template {
    /// All elements in the template. The root element is always at index 0.
    pub root: Element,
    /// Definition span.
    pub location: SourceLocation,
}

impl Template {
    /// Returns all child elements and their descendants, in depth-first order.
    pub fn elements(&self) -> Vec<&Element> {
        let mut elements = Vec::new();
        self.root.collect_elements(&mut elements);
        elements
    }
}

impl Element {
    /// Returns all child elements and their descendants, in depth-first order.
    pub fn collect_elements<'a>(&'a self, out: &mut Vec<&'a Element>) {
        out.push(&self);
        for child in &self.children {
            child.collect_elements(out);
        }
    }
}