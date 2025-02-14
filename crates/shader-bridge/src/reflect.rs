//! Generates rust code from slang reflection data.
use crate::reflect::Error::BindgenError;
use heck::ToSnakeCase;
use proc_macro2::TokenStream;
use quote::{format_ident, quote, TokenStreamExt};
use slang::reflection::VariableLayout;
use std::collections::HashMap;
use tracing::error;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("slang error: {0:?}")]
    SlangError(slang::Error),
    #[error("I/O error: {0:?}")]
    IoError(#[from] std::io::Error),
    #[error("binding generation error: {0:?}")]
    BindgenError(String),
}

impl From<slang::Error> for Error {
    fn from(err: slang::Error) -> Self {
        Error::SlangError(err)
    }
}

pub struct Ctx {
    /// Visibility of the generated types & fields.
    visibility: syn::Visibility,
    /// Map of user-defined types (structs) that have already been translated.
    user_defined_types: HashMap<String, syn::Ident>,
    /// Counter for generating unique names.
    counter: usize,
    output: TokenStream,
    /// Errors during translation.
    errors: Vec<Error>,
    /// Vector type remapping.
    ///
    /// By default, vector types are translated to `[T; N]` arrays, but if the `(scalar, count)`
    /// pair is present in this map, the vector type will be translated to the specified type.
    vector_type_map: HashMap<(slang::ScalarType, u8), TokenStream>,
    /// The type for device pointers.
    device_ptr_type: TokenStream,
}

impl Ctx {
    pub fn new() -> Self {
        Self {
            visibility: syn::parse_quote!(pub),
            user_defined_types: HashMap::new(),
            counter: 0,
            output: TokenStream::new(),
            errors: Vec::new(),
            vector_type_map: HashMap::new(),
            device_ptr_type: quote! { PhysicalAddress },
        }
    }

    fn translate_scalar_type(&mut self, ty: slang::ScalarType) -> TokenStream {
        match ty {
            slang::ScalarType::None => {
                quote!(())
            }
            slang::ScalarType::Void => {
                quote!(())
            }
            // FIXME: bool types shouldn't end up in shader interfaces
            slang::ScalarType::Bool => {
                quote!(bool)
            }
            slang::ScalarType::Int32 => {
                quote!(i32)
            }
            slang::ScalarType::Uint32 => {
                quote!(u32)
            }
            slang::ScalarType::Int64 => {
                quote!(i64)
            }
            slang::ScalarType::Uint64 => {
                quote!(u64)
            }
            slang::ScalarType::Float16 => {
                self.errors
                    .push(BindgenError("unsupported scalar type: `float16`".to_owned()));
                quote!(())
            }
            slang::ScalarType::Float32 => {
                quote!(f32)
            }
            slang::ScalarType::Float64 => {
                quote!(f64)
            }
            slang::ScalarType::Int8 => {
                quote!(i8)
            }
            slang::ScalarType::Uint8 => {
                quote!(u8)
            }
            slang::ScalarType::Int16 => {
                quote!(i16)
            }
            slang::ScalarType::Uint16 => {
                quote!(u16)
            }
            // FIXME: ???
            slang::ScalarType::Intptr => {
                quote!(isize)
            }
            // FIXME: ???
            slang::ScalarType::Uintptr => {
                quote!(usize)
            }
        }
    }

    fn translate_vector_type(&mut self, ty: &slang::reflection::TypeLayout) -> TokenStream {
        let scalar_ty = ty.scalar_type();
        let count = ty.element_count();
        if let Some(ty) = self.vector_type_map.get(&(scalar_ty, count as u8)) {
            ty.clone()
        } else {
            let scalar_ty = self.translate_scalar_type(scalar_ty);
            quote! { [#scalar_ty; #count] }
        }
    }

    fn translate_matrix_type(&mut self, ty: &slang::reflection::TypeLayout) -> TokenStream {
        let scalar_ty = ty.scalar_type();
        let rows = ty.row_count() as usize;
        let cols = ty.column_count() as usize;
        let scalar_ty = self.translate_scalar_type(scalar_ty);
        quote! { [[#scalar_ty; #cols]; #rows] }
    }

    fn translate_struct_type(&mut self, ty: &slang::reflection::TypeLayout) -> syn::Ident {
        let mut field_names = Vec::new();
        let mut field_offsets = Vec::new();
        let mut field_types = Vec::new();

        for (field_index, field) in ty.fields().enumerate() {
            let var = field.variable();
            let name = if let Some(name) = var.name() {
                let snake_name = name.to_snake_case();
                format_ident!("{snake_name}")
            } else {
                format_ident!("anon_{field_index}")
            };
            let tty = self.tr_type(field.type_layout());
            field_names.push(name);
            field_offsets.push(field.offset(ty.parameter_category()));
            field_types.push(tty);
        }

        let ident;
        if let Some(name) = ty.name() {
            ident = format_ident!("{name}");
            self.user_defined_types.insert(name.to_owned(), ident.clone());
        } else {
            ident = format_ident!("anon_{}", self.counter);
            self.counter += 1;
        };

        // offset check
        let size = ty.size(ty.parameter_category());

        let vis = &self.visibility;
        self.output.append_all(quote! {
            #[derive(Clone, Copy)]
            #[repr(C)]
            #vis struct #ident {
                #(#vis #field_names: #field_types),*
            }

            impl #ident {
                const _LAYOUT_CHECK: () = {
                    #(assert!( #field_offsets == ::core::mem::offset_of!(#ident, #field_names));)*
                    assert!( ::core::mem::size_of::<#ident>() == #size);
                };
            }
        });
        ident
    }

    fn translate_pointer_type(&mut self, ty: &slang::reflection::TypeLayout) -> TokenStream {
        let pointee = self.tr_type(ty.element_type_layout());
        let ptr_ty_ctor = &self.device_ptr_type;
        quote! {  #ptr_ty_ctor < #pointee > }
    }

    fn type_ref(&mut self, ty: &slang::reflection::TypeLayout) -> syn::Ident {
        if let Some(name) = ty.name() {
            if let Some(ident) = self.user_defined_types.get(name).cloned() {
                return ident;
            }
        }
        self.translate_struct_type(ty)
    }

    /// Translates a slang primitive type to a rust type.
    fn tr_type(&mut self, ty: &slang::reflection::TypeLayout) -> TokenStream {
        //let unit = ty.parameter_category();

        match ty.kind() {
            slang::TypeKind::None => {
                quote! { () }
            }
            slang::TypeKind::Struct => {
                let tyref = self.type_ref(ty);
                quote! { #tyref }
            }
            slang::TypeKind::Array => {
                // issue: constant references are resolved to their values, so we lose some
                // information in the bindings, but that's not too bad.
                let count = ty.element_count();
                let element_ty = self.tr_type(ty.element_type_layout());
                quote! { [#element_ty; #count] }
            }
            slang::TypeKind::Matrix => self.translate_matrix_type(ty),
            slang::TypeKind::Vector => self.translate_vector_type(ty),
            slang::TypeKind::Scalar => self.translate_scalar_type(ty.scalar_type()),
            slang::TypeKind::Pointer => self.translate_pointer_type(ty),
            slang::TypeKind::ConstantBuffer
            | slang::TypeKind::Resource
            | slang::TypeKind::SamplerState
            | slang::TypeKind::TextureBuffer
            | slang::TypeKind::ShaderStorageBuffer
            | slang::TypeKind::ParameterBlock
            | slang::TypeKind::GenericTypeParameter
            | slang::TypeKind::Interface
            | slang::TypeKind::OutputStream
            | slang::TypeKind::MeshOutput
            | slang::TypeKind::Specialized
            | slang::TypeKind::Feedback
            | slang::TypeKind::DynamicResource
            | slang::TypeKind::Count => {
                self.errors
                    .push(BindgenError(format!("unsupported type kind: {:?}", ty.kind())));
                quote! { () }
            }
        }
    }

    fn generate_interface_var(&mut self, var: &VariableLayout) {

        eprintln!("var: {:?}", var.ty().name());

        // Ignore varying inputs/outputs because they don't have a memory layout on the host
        if var.category() == slang::ParameterCategory::VaryingInput
            || var.category() == slang::ParameterCategory::VaryingOutput
        {
            return;
        }

        let ty = var.type_layout();

        match ty.kind() {
            slang::TypeKind::None => {}
            slang::TypeKind::Struct
            | slang::TypeKind::Array
            | slang::TypeKind::Matrix
            | slang::TypeKind::Vector
            | slang::TypeKind::Scalar => {
                let _ = self.tr_type(ty);
            }
            slang::TypeKind::ConstantBuffer => {
                let _ = self.tr_type(ty.element_type_layout());
            }
            slang::TypeKind::Resource => {
                let _ = self.tr_type(ty.element_type_layout());
            }
            slang::TypeKind::SamplerState => {}
            slang::TypeKind::TextureBuffer => {}
            slang::TypeKind::ShaderStorageBuffer => {}
            slang::TypeKind::ParameterBlock => {
                eprintln!("unimplemented parameter kind: {:?}", ty.kind());
            }
            slang::TypeKind::GenericTypeParameter => {}
            slang::TypeKind::Interface => {}
            slang::TypeKind::OutputStream => {}
            slang::TypeKind::MeshOutput => {}
            slang::TypeKind::Specialized => {}
            slang::TypeKind::Feedback => {}
            slang::TypeKind::Pointer => {}
            slang::TypeKind::DynamicResource => {}
            _ => {}
        }
    }

    /// Scans all types used in the shader interface and generates rust code for them.
    pub fn generate_interface(&mut self, reflection: &slang::reflection::Shader) {
        // scan all parameters
        for param in reflection.parameters() {
            self.generate_interface_var(param);
        }

        // scan uniforms in entry points arguments
        for entry_point in reflection.entry_points() {
            for param in entry_point.parameters() {
                self.generate_interface_var(param);
            }
        }

        // generate compile_error for each error
        for error in &self.errors {
            let error_str = error.to_string();
            self.output.append_all(quote! {
                ::std::compile_error!(#error_str);
            });
        }
    }

    pub fn finish(self) -> TokenStream {
        self.output
    }
}


/*
fn generate_rust_bindings_inner(
    session: &Session,
    modules: &[Module],
    device_address_type: &str,
) -> Result<TokenStream, Error> {
    let mut ctx = Ctx {
        visibility: syn::parse_quote!(pub),
        user_defined_types: HashMap::new(),
        counter: 0,
        output: TokenStream::new(),
        errors: Vec::new(),
        vector_type_map: HashMap::new(),
        device_ptr_type: syn::parse_str(device_address_type).unwrap(),
    };

    generate_common_definitions(&mut ctx.output);

    // --- generate rust bindings ---
    {
        // create composite of all modules and all their entry points for generating the reflection
        let mut components = Vec::new();
        let mut all_entry_point_count = 0;
        for module in modules {
            components.push(module.downcast().clone());
            let entry_point_count = module.entry_point_count();
            for i in 0..entry_point_count {
                let entry_point = module.entry_point_by_index(i).unwrap();
                components.push(entry_point.downcast().clone());
                all_entry_point_count += 1;
            }
        }
        let program = session.create_composite_component_type(&components)?;

        // generate rust bindings
        let reflection = program.layout(0)?;
        ctx.generate_interface(reflection);
    }

    Ok(ctx.output)
}

pub fn generate_rust_bindings(session: &Session, modules: &[Module], device_address_type: &str) -> TokenStream {
    generate_rust_bindings_inner(&session, modules, device_address_type).unwrap_or_else(|err| {
        let err_str = err.to_string();
        quote! {
                ::std::compile_error!(#err_str);
            }
    })
}*/
