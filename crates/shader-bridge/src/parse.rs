use quote::{ToTokens};
use std::{
    fmt,
    fmt::{Display, Formatter},
};
use syn::{
    parse::{Result},
    Attribute, Error, Item, Meta,
};
use syn::spanned::Spanned;

/*
/// Constant integer value.
#[derive(Clone, PartialEq, Eq)]
pub(crate) enum ConstIntVal {
    Literal(i64),
    Named(String),
}

impl ConstIntVal {
    fn parse(expr: &syn::Expr) -> Result<ConstIntVal> {
        match expr {
            syn::Expr::Lit(syn::Lit::Int(lit)) => {
                let value: i64 = lit.base10_parse()?;
                Ok(ConstIntVal::Literal(value))
            }
            syn::Expr::Path(path) if path.qself.is_none() && path.path.segments.len() == 1 && path.path.segments[0].arguments.is_none() => {
                Ok(ConstIntVal::Named(path.path.segments[0].ident.to_string()))
            }
            _ => Err(Error::new_spanned(expr, "unsupported constant expression")),
        }
    }
}*/

#[derive(Clone)]
pub(crate) enum Type {
    F32,
    F64,
    I32,
    U32,
    Bool,
    I64,
    U64,
    Vec2,
    Vec3,
    Vec4,
    IVec2,
    IVec3,
    IVec4,
    UVec2,
    UVec3,
    UVec4,
    Mat2,
    Mat3,
    Mat4,
    Array { elem_ty: Box<Type>, size: Box<syn::Expr> },
    Slice(Box<Type>),
    Ref(String),
    DeviceAddress(Box<Type>),
    ImageHandle,
    Texture2DHandleRange,
    SamplerHandle,
}

impl fmt::Debug for Type {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Type::F32 => write!(f, "f32"),
            Type::F64 => write!(f, "f64"),
            Type::I32 => write!(f, "i32"),
            Type::U32 => write!(f, "u32"),
            Type::Bool => write!(f, "bool"),
            Type::I64 => write!(f, "i64"),
            Type::U64 => write!(f, "u64"),
            Type::Vec2 => write!(f, "Vec2"),
            Type::Vec3 => write!(f, "Vec3"),
            Type::Vec4 => write!(f, "Vec4"),
            Type::IVec2 => write!(f, "IVec2"),
            Type::IVec3 => write!(f, "IVec3"),
            Type::IVec4 => write!(f, "IVec4"),
            Type::UVec2 => write!(f, "UVec2"),
            Type::UVec3 => write!(f, "UVec3"),
            Type::UVec4 => write!(f, "UVec4"),
            Type::Mat2 => write!(f, "Mat2"),
            Type::Mat3 => write!(f, "Mat3"),
            Type::Mat4 => write!(f, "Mat4"),
            Type::Array { elem_ty, .. } => write!(f, "[{:?}; ...]", elem_ty),
            Type::Ref(name) => write!(f, "{}", name),
            Type::DeviceAddress(elem_ty) => write!(f, "DeviceAddress<{:?}>", elem_ty),
            Type::ImageHandle => write!(f, "ImageHandle"),
            Type::Texture2DHandleRange => write!(f, "Texture2DHandleRange"),
            Type::SamplerHandle => write!(f, "SamplerHandle"),
            Type::Slice(elem_ty) => write!(f, "Slice<{:?}>", elem_ty),
        }
    }
}

impl Type {
    /// If the type contains a reference to a struct type, return the name of the struct.
    fn has_struct_ref(&self) -> Option<&str> {
        match self {
            Type::Ref(name) => Some(name),
            Type::DeviceAddress(inner_ty) => inner_ty.has_struct_ref(),
            Type::Array { elem_ty, .. } => elem_ty.has_struct_ref(),
            Type::Slice(elem_ty) => elem_ty.has_struct_ref(),
            _ => None,
        }
    }

    /// If the type contains a reference to a struct type via DeviceAddress, return the name of the struct.
    fn has_struct_buffer_ref(&self) -> Option<&str> {
        match self {
            Type::DeviceAddress(inner_ty) => inner_ty.has_struct_ref(),
            _ => None,
        }
    }

    fn parse(ty: &syn::Type) -> Result<Type> {
        match ty {
            syn::Type::Path(p) => {
                if p.qself.is_some() {
                    return Err(Error::new_spanned(p, "qualified paths are not supported"));
                }
                let p = &p.path;
                if p.is_ident("f32") {
                    Ok(Type::F32)
                } else if p.is_ident("f64") {
                    Ok(Type::F64)
                } else if p.is_ident("i32") {
                    Ok(Type::I32)
                } else if p.is_ident("u32") {
                    Ok(Type::U32)
                } else if p.is_ident("usize") {
                    // usize is treated as u32
                    // We can't just forbid using usize in constants because that's the type of array length
                    // constant expressions. We'd end up with things like:
                    //
                    //      const N: u32 = 3;
                    //      struct Uniforms {
                    //          data: [f32; N as usize],
                    //      }
                    //
                    // but then we'd need to handle `as` expressions when translating array types,
                    // which we'd like to avoid.
                    Ok(Type::U32)
                } else if p.is_ident("isize") {
                    // Same reasoning as usize
                    Ok(Type::I32)
                } else if p.is_ident("u32") {
                    Ok(Type::U32)
                } else if p.is_ident("bool") {
                    Ok(Type::Bool)
                } else if p.is_ident("i64") {
                    Ok(Type::I64)
                } else if p.is_ident("u64") {
                    Ok(Type::U64)
                } else if p.is_ident("Vec2") {
                    Ok(Type::Vec2)
                } else if p.is_ident("Vec3") {
                    Ok(Type::Vec3)
                } else if p.is_ident("Vec4") {
                    Ok(Type::Vec4)
                } else if p.is_ident("IVec2") {
                    Ok(Type::IVec2)
                } else if p.is_ident("IVec3") {
                    Ok(Type::IVec3)
                } else if p.is_ident("IVec4") {
                    Ok(Type::IVec4)
                } else if p.is_ident("UVec2") {
                    Ok(Type::UVec2)
                } else if p.is_ident("UVec3") {
                    Ok(Type::UVec3)
                } else if p.is_ident("UVec4") {
                    Ok(Type::UVec4)
                } else if p.is_ident("Mat2") {
                    Ok(Type::Mat2)
                } else if p.is_ident("Mat3") {
                    Ok(Type::Mat3)
                } else if p.is_ident("Mat4") {
                    Ok(Type::Mat4)
                } else {
                    if p.is_ident("SamplerHandle") {
                        Ok(Type::SamplerHandle)
                    } else if p.is_ident("ImageHandle") {
                        Ok(Type::ImageHandle)
                    } else if p.is_ident("Texture2DHandleRange") {
                        Ok(Type::Texture2DHandleRange)
                    } else if p.segments.len() == 1 {
                        let seg = &p.segments[0];
                        if seg.ident == "DeviceAddress" {
                            // buffer reference
                            let elem_ty = match seg.arguments {
                                syn::PathArguments::AngleBracketed(ref args) => {
                                    if args.args.len() != 1 {
                                        return Err(Error::new_spanned(args, "expected a single type argument"));
                                    }
                                    let arg = args.args.first().unwrap();
                                    match arg {
                                        syn::GenericArgument::Type(ty) => Type::parse(ty)?,
                                        _ => return Err(Error::new_spanned(arg, "expected a type argument")),
                                    }
                                }
                                _ => return Err(Error::new_spanned(seg, "expected a type argument")),
                            };
                            Ok(Type::DeviceAddress(Box::new(elem_ty)))
                        } else {
                            // struct reference
                            if !seg.arguments.is_empty() {
                                return Err(Error::new_spanned(seg, "generic types are not supported"));
                            } else {
                                Ok(Type::Ref(seg.ident.to_string()))
                            }
                        }
                    } else {
                        Err(Error::new_spanned(p, "unsupported type"))
                    }
                }
            }
            syn::Type::Array(a) => {
                let elem_ty = Type::parse(&a.elem)?;
                Ok(Type::Array {
                    elem_ty: Box::new(elem_ty),
                    size: Box::new(a.len.clone()),
                })
            }
            syn::Type::Slice(slice) => {
                let elem_ty = Type::parse(&slice.elem)?;
                Ok(Type::Slice(Box::new(elem_ty)))
            }
            _ => Err(Error::new_spanned(ty, "unsupported type")),
        }
    }

    /// Replaces arrays of {float,int,uint}, and of size {2,3,4} with their corresponding
    /// vector types.
    ///
    /// It's OK to do so because all of our shaders use the GLSL scalar layout, which makes
    /// `[f32; N]` equivalent to `vecN`.
    fn vectorize(self) -> Type {
        match self {
            // please give me box patterns for Christmas
            Type::Array {
                elem_ty: ref inner_ty,
                ref size,
            } => {
                let size = match **size {
                    syn::Expr::Lit(ref lit) => {
                        match lit.lit {
                            syn::Lit::Int(ref lit) => lit.base10_parse::<usize>().unwrap(),
                            _ => return self,
                        }
                    }
                    _ => return self,
                };

                match **inner_ty {
                    Type::F32 => match size {
                        2 => Type::Vec2,
                        3 => Type::Vec3,
                        4 => Type::Vec4,
                        _ => self.clone(),
                    },
                    Type::I32 => match size {
                        2 => Type::IVec2,
                        3 => Type::IVec3,
                        4 => Type::IVec4,
                        _ => self.clone(),
                    },
                    Type::U32 => match size {
                        2 => Type::UVec2,
                        3 => Type::UVec3,
                        4 => Type::UVec4,
                        _ => self.clone(),
                    },
                    _ => self.clone(),
                }
            }
            _ => self
        }
    }
}

pub(crate) struct Field {
    pub(crate) name: syn::Ident,
    pub(crate) ty: Type,
}

pub(crate) struct Struct {
    pub(crate) attrs: Attrs,
    pub(crate) name: syn::Ident,
    pub(crate) fields: Vec<Field>,
    /// True if some other field references this type in a pointer (BufferHandle)
    pub(crate) buffer_ref: bool,
}

impl Struct {
    fn parse(item_struct: &syn::ItemStruct) -> Result<Self> {
        let attrs = parse_attrs(&item_struct.attrs)?;
        if !(attrs.repr_c || attrs.repr_transparent) {
            return Err(Error::new(item_struct.span(), "structs must be repr(C) or repr(transparent) in order to be shared with shaders"));
        }
        let name = item_struct.ident.clone();
        let fields = item_struct
            .fields
            .iter()
            .map(|field| {
                let name = field.ident.clone().unwrap();
                let ty = Type::parse(&field.ty)?.vectorize();
                Ok(Field { name, ty })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Struct { attrs, name, fields, buffer_ref: false })
    }
}

pub(crate) struct Const {
    pub(crate) attrs: Attrs,
    pub(crate) name: syn::Ident,
    pub(crate) ty: Type,
    pub(crate) value: Box<syn::Expr>,
}

impl Const {
    fn parse(item_const: syn::ItemConst) -> Result<Self> {
        let attrs = parse_attrs(&item_const.attrs)?;
        let name = item_const.ident.clone();
        let ty = Type::parse(&item_const.ty)?;
        Ok(Const {
            attrs,
            name,
            ty,
            value: item_const.expr,
        })
    }
}

pub(crate) enum BridgeItem {
    Struct(Struct),
    Const(Const),
}

pub(crate) struct BridgeModule {
    /// The module's items
    pub(crate) items: Vec<BridgeItem>,
}

pub(crate) fn parse_file(file: syn::File) -> (BridgeModule, Errors) {
    let mut errs = Errors::new();
    let mut items = vec![];
    for item in file.items {
        match item {
            Item::Use(_) => {}
            Item::Struct(s) => match Struct::parse(&s) {
                Ok(s) => items.push(BridgeItem::Struct(s)),
                Err(err) => errs.push_err(err),
            },
            Item::Const(c) => match Const::parse(c) {
                Ok(c) => items.push(BridgeItem::Const(c)),
                Err(err) => errs.push_err(err),
            },
            _ => {
                errs.error(&item, "unsupported item in module");
            }
        }
    }

    // Check for buffer references
    let mut buffer_references = Vec::new();
    for buf in items.iter() {
        match buf {
            BridgeItem::Struct(s) => {
                for field in &s.fields {
                    if let Some(struct_ref) = field.ty.has_struct_buffer_ref() {
                        for (j, buf) in items.iter().enumerate() {
                            if let BridgeItem::Struct(ref s) = buf {
                                if s.name == struct_ref {
                                    buffer_references.push(j);
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    for i in buffer_references {
        if let BridgeItem::Struct(ref mut s) = items[i] {
            s.buffer_ref = true;
        }
    }

    (BridgeModule { items }, errs)
}

pub(crate) struct Errors {
    pub(crate) errors: Vec<Error>,
}

impl Errors {
    pub(crate) fn new() -> Self {
        Errors { errors: Vec::new() }
    }

    pub(crate) fn error(&mut self, sp: impl ToTokens, msg: impl Display) {
        self.errors.push(Error::new_spanned(sp, msg));
    }

    pub(crate) fn push_err(&mut self, error: Error) {
        self.errors.push(error);
    }
}

#[derive(Default)]
pub(crate) struct Attrs {
    pub doc: String,
    pub repr_c: bool,
    pub repr_transparent: bool,
}

mod kw {
    syn::custom_keyword!(hidden);
}

fn parse_doc_attribute(meta: &Meta) -> Result<String> {
    match meta {
        Meta::NameValue(meta) => {
            if let syn::Expr::Lit(expr) = &meta.value {
                if let syn::Lit::Str(lit) = &expr.lit {
                    return Ok(lit.value());
                }
            }
        }
        Meta::List(meta) => {
            meta.parse_args::<kw::hidden>()?;
            return Ok(String::new());
        }
        Meta::Path(_) => {}
    }
    Err(Error::new_spanned(meta, "unsupported doc attribute"))
}

fn parse_attrs(attrs: &[Attribute]) -> Result<Attrs> {
    let mut doc = String::new();
    let mut repr_c = false;
    let mut repr_transparent = false;

    for attr in attrs {
        let attr_path = attr.path();
        if attr_path.is_ident("doc") {
            doc.push_str(&parse_doc_attribute(&attr.meta)?);
        }
        if attr_path.is_ident("repr") {
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("C") {
                    repr_c = true;
                } else if meta.path.is_ident("transparent") {
                    repr_transparent = true;
                }
                Ok(())
            })?;
        }
    }

    Ok(Attrs { doc, repr_c, repr_transparent })
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::ExprLit;

    fn parse_type(ty: &str) -> Type {
        Type::parse(&syn::parse_str(ty).unwrap()).unwrap()
    }

    #[test]
    fn test_types() {
        assert!(matches!(parse_type("f32"), Type::F32));
        assert!(matches!(parse_type("f64"), Type::F64));
        assert!(matches!(parse_type("i32"), Type::I32));
        assert!(matches!(parse_type("u32"), Type::U32));
        assert!(matches!(parse_type("bool"), Type::Bool));
        assert!(matches!(parse_type("i64"), Type::I64));
        assert!(matches!(parse_type("u64"), Type::U64));

        assert!(matches!(parse_type("Vec2"), Type::Vec2));
        assert!(matches!(parse_type("Vec3"), Type::Vec3));
        assert!(matches!(parse_type("Vec4"), Type::Vec4));
        assert!(matches!(parse_type("IVec2"), Type::IVec2));
        assert!(matches!(parse_type("IVec3"), Type::IVec3));
        assert!(matches!(parse_type("IVec4"), Type::IVec4));
        assert!(matches!(parse_type("UVec2"), Type::UVec2));
        assert!(matches!(parse_type("UVec3"), Type::UVec3));
        assert!(matches!(parse_type("UVec4"), Type::UVec4));
        assert!(matches!(parse_type("Mat2"), Type::Mat2));
        assert!(matches!(parse_type("Mat3"), Type::Mat3));
        assert!(matches!(parse_type("Mat4"), Type::Mat4));

        // by god testing is atrocious if you don't have a PartialEq impl
        let arr_ty = parse_type("[f32; 3]");
        match arr_ty {
            Type::Array { elem_ty, size } => match *elem_ty {
                Type::F32 => match *size {
                    syn::Expr::Lit(ExprLit {
                                       lit: syn::Lit::Int(lit), ..
                                   }) => {
                        assert_eq!(lit.base10_parse::<u64>().unwrap(), 3);
                    }
                    _ => panic!("expected integer literal"),
                },
                _ => panic!("expected f32 type"),
            },
            _ => panic!("expected array type"),
        }

        let arr_ty = parse_type("[[f32; 3]; 2]");
        match arr_ty {
            Type::Array { elem_ty, size } => {
                match *elem_ty {
                    Type::Array { elem_ty, size } => match *elem_ty {
                        Type::F32 => match *size {
                            syn::Expr::Lit(ExprLit {
                                               lit: syn::Lit::Int(lit), ..
                                           }) => {
                                assert_eq!(lit.base10_parse::<u64>().unwrap(), 3);
                            }
                            _ => panic!("expected integer literal"),
                        },
                        _ => panic!("expected f32 type"),
                    },
                    _ => panic!("expected array type"),
                }
                match *size {
                    syn::Expr::Lit(ExprLit {
                                       lit: syn::Lit::Int(lit), ..
                                   }) => {
                        assert_eq!(lit.base10_parse::<u64>().unwrap(), 2);
                    }
                    _ => panic!("expected integer literal"),
                }
            }
            _ => panic!("expected array type"),
        }

        assert!(matches!(parse_type("BufferHandle<f32>"), Type::DeviceAddress(b) if matches!(*b, Type::F32)));
        assert!(matches!(parse_type("BufferHandle<Vec2>"), Type::DeviceAddress(b) if matches!(*b, Type::Vec2)));
        assert!(matches!(parse_type("BufferHandle<BufferHandle<f32>>"), Type::DeviceAddress(b) if matches!(*b, Type::DeviceAddress(ref b) if matches!(**b, Type::F32))));

        assert!(matches!(parse_type("SomeStruct"), Type::Ref(r) if r == "SomeStruct"));
    }
}
