use crate::parse::{Attrs, BridgeItem, BridgeModule, Const, Struct, Type};
use heck::ToLowerCamelCase;
use quote::ToTokens;
use std::{fmt, io::Write};

type Output = Vec<u8>;

struct DisplayTypeGLSL<'a>(&'a Type);

// This reinterprets rust code verbatim into GLSL code. Obviously this won't work for most things
// but for simple expressions that do not involve casts, number suffixes, compound paths, function calls, or method calls, it should work.
fn expr_to_string(expr: &syn::Expr) -> String {
    expr.to_token_stream().to_string()
}

impl fmt::Display for DisplayTypeGLSL<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Type::F32 => write!(f, "float"),
            Type::F64 => write!(f, "double"),
            Type::I8 => write!(f, "int8_t"),
            Type::U8 => write!(f, "uint8_t"),
            Type::I16 => write!(f, "int16_t"),
            Type::U16 => write!(f, "uint16_t"),
            Type::I32 => write!(f, "int"),
            Type::U32 => write!(f, "uint"),
            Type::Vec2 => write!(f, "vec2"),
            Type::Vec3 => write!(f, "vec3"),
            Type::Vec4 => write!(f, "vec4"),
            Type::IVec2 => write!(f, "ivec2"),
            Type::IVec3 => write!(f, "ivec3"),
            Type::IVec4 => write!(f, "ivec4"),
            Type::UVec2 => write!(f, "uvec2"),
            Type::UVec3 => write!(f, "uvec3"),
            Type::UVec4 => write!(f, "uvec4"),
            Type::I8Vec2 => write!(f, "i8vec2"),
            Type::I8Vec3 => write!(f, "i8vec3"),
            Type::I8Vec4 => write!(f, "i8vec4"),
            Type::U8Vec2 => write!(f, "u8vec2"),
            Type::U8Vec3 => write!(f, "u8vec3"),
            Type::U8Vec4 => write!(f, "u8vec4"),
            Type::I16Vec2 => write!(f, "i16vec2"),
            Type::I16Vec3 => write!(f, "i16vec3"),
            Type::I16Vec4 => write!(f, "i16vec4"),
            Type::U16Vec2 => write!(f, "u16vec2"),
            Type::U16Vec3 => write!(f, "u16vec3"),
            Type::U16Vec4 => write!(f, "u16vec4"),
            Type::Mat2 => write!(f, "mat2"),
            Type::Mat3 => write!(f, "mat3"),
            Type::Mat4 => write!(f, "mat4"),
            Type::Bool => write!(f, "bool"),
            Type::I64 => write!(f, "int64_t"),
            Type::U64 => write!(f, "uint64_t"),
            Type::DeviceAddress(inner_ty) => {
                // just assume this is a buffer reference
                match **inner_ty {
                    Type::Slice(ref elem_ty) => write!(f, "{}Slice", DisplayTypeGLSL(elem_ty)),
                    _ => write!(f, "{}Ptr", DisplayTypeGLSL(inner_ty)),
                }
            }
            // TODO multidimensional arrays will be fucked up, don't use them
            Type::Array { elem_ty, size } => write!(f, "{}[{}]", DisplayTypeGLSL(elem_ty), expr_to_string(size)),
            Type::Ref(r) => write!(f, "{r}"),
            Type::ImageHandle => write!(f, "image2DHandle"),
            Type::SamplerHandle => write!(f, "samplerHandle"),
            Type::Texture2DHandleRange => write!(f, "texture2DRange"),
            Type::Slice(elem_ty) => {
                // just assume this is a buffer reference
                write!(f, "{}", DisplayTypeGLSL(elem_ty))
            }
        }
    }
}

fn write_docs(attrs: &Attrs, out: &mut Output) {
    if !attrs.doc.is_empty() {
        for line in attrs.doc.lines() {
            writeln!(out, "// {line}").unwrap();
        }
    }
}

fn write_buffer_reference_forward_decl(s: &Struct, array: bool, out: &mut Output) {
    let name = &s.name;
    let suffix = if array { "Slice" } else { "Ptr" };
    writeln!(out, "layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer {name}{suffix};").unwrap();
}

fn write_buffer_reference(s: &Struct, array: bool, out: &mut Output) {
    let name = &s.name;
    let suffix = if array { "Slice" } else { "Ptr" };
    let array = if array { "[]" } else { "" };
    write!(out, "layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer {name}{suffix} {{").unwrap();
    write!(out, "{name}{array} d;").unwrap();
    writeln!(out, "}};").unwrap();
}

fn write_struct(s: &Struct, out: &mut Output) {
    if s.buffer_ref {
        write_buffer_reference_forward_decl(s, false, out);
        write_buffer_reference_forward_decl(s, true, out);
        writeln!(out).unwrap();
    }
    write_docs(&s.attrs, out);
    let name = &s.name;
    writeln!(out, "struct {name} {{").unwrap();
    for f in &s.fields {
        let name = &f.name.to_string().to_lower_camel_case();
        let ty = DisplayTypeGLSL(&f.ty);
        writeln!(out, "    {ty} {name};").unwrap();
    }
    writeln!(out, "}};\n").unwrap();
    if s.buffer_ref {
        write_buffer_reference(s, false, out);
        write_buffer_reference(s, true, out);
    }
}

fn write_const(c: &Const, out: &mut Output) {
    write_docs(&c.attrs, out);
    let ty = DisplayTypeGLSL(&c.ty);
    let name = &c.name;
    let value = expr_to_string(&c.value);
    writeln!(out, "const {ty} {name} = {value};").unwrap();
}

fn write_item(item: &BridgeItem, out: &mut Output) {
    match item {
        BridgeItem::Struct(s) => {
            write_struct(s, out);
        }
        BridgeItem::Const(c) => {
            write_const(c, out);
        }
    }
}

fn write_primitive_buffer_references(out: &mut Output) {
    let contents = r#"
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer intPtr { int d; };
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer uintPtr { uint d; };
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer floatPtr { float d; };
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer vec2Ptr { vec2 d; };
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer vec3Ptr { vec3 d; };
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer vec4Ptr { vec4 d; };
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer ivec2Ptr { ivec2 d; };
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer ivec3Ptr { ivec3 d; };
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer ivec4Ptr { ivec4 d; };
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer uvec2Ptr { uvec2 d; };
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer uvec3Ptr { uvec3 d; };
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer uvec4Ptr { uvec4 d; };
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer mat2Ptr { mat2 d; };
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer mat3Ptr { mat3 d; };
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer mat4Ptr { mat4 d; };
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer intSlice { int[] d; };
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer uintSlice { uint[] d; };
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer floatSlice { float[] d; };
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer vec2Slice { vec2[] d; };
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer vec3Slice { vec3[] d; };
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer vec4Slice { vec4[] d; };
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer ivec2Slice { ivec2[] d; };
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer ivec3Slice { ivec3[] d; };
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer ivec4Slice { ivec4[] d; };
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer uvec2Slice { uvec2[] d; };
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer uvec3Slice { uvec3[] d; };
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer uvec4Slice { uvec4[] d; };
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer mat2Slice { mat2[] d; };
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer mat3Slice { mat3[] d; };
layout(buffer_reference, scalar, buffer_reference_align=8) coherent buffer mat4Slice { mat4[] d; };
layout(buffer_reference, scalar, buffer_reference_align=4) coherent buffer image2DHandleSlice { image2DHandle[] d; };


"#;
    write!(out, "{}", contents).unwrap();
}

pub(crate) fn write_module(m: &BridgeModule, out: &mut Output) {
    write_primitive_buffer_references(out);
    for item in &m.items {
        write_item(item, out);
        write!(out, "\n\n").unwrap();
    }
}
