//! Generates rust code from slang reflection data.
use anyhow::anyhow;
use proc_macro2::TokenStream;
use quote::{format_ident, quote, TokenStreamExt};
use slang::reflection::VariableLayout;
use slang::{Downcast, GlobalSession, Module, Session};
use std::collections::HashMap;
use std::ffi::{CString, OsStr};
use std::mem;
use std::path::Path;
use std::str::FromStr;
use heck::ToSnakeCase;
use tracing::{error, info, trace, warn};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("slang error: {0:?}")]
    SlangError(slang::Error),
    #[error("I/O error: {0:?}")]
    IoError(#[from] std::io::Error),
}

impl From<slang::Error> for Error {
    fn from(err: slang::Error) -> Self {
        Error::SlangError(err)
    }
}

pub struct Ctx {
    global_session: GlobalSession,
    session: Session,
    modules: Vec<Module>,
    /// Visibility of the generated types & fields.
    visibility: syn::Visibility,
    /// Map of user-defined types (structs) that have already been translated.
    user_defined_types: HashMap<String, syn::Ident>,
    /// Counter for generating unique names.
    counter: usize,
    output: TokenStream,
    /// Errors during translation.
    errors: Vec<String>,
    /// Vector type remapping.
    ///
    /// By default, vector types are translated to `[T; N]` arrays, but if the `(scalar, count)`
    /// pair is present in this map, the vector type will be translated to the specified type.
    vector_type_map: HashMap<(slang::ScalarType, u8), TokenStream>,
    /// The type for device pointers.
    device_ptr_type: TokenStream,
}

impl Ctx {
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
                self.errors.push("unsupported scalar type: `float16`".to_owned());
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
        let rows = ty.row_count();
        let cols = ty.column_count();
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
            #[derive(Debug, Clone, Copy)]
            #[repr(C)]
            #vis struct #ident {
                #(#field_names: #field_types),*
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
                self.errors.push(format!("unsupported type kind: {:?}", ty.kind()));
                quote! { () }
            }
        }
    }

    fn write_interface_var(&mut self, var: &VariableLayout) {
        // Ignore varying inputs/outputs because they don't have a memory layout on the host
        if var.category() == slang::ParameterCategory::VaryingInput
            || var.category() == slang::ParameterCategory::VaryingOutput
        {
            return;
        }

        let ty = var.type_layout();
        let var = var.variable();
        let name = var.name().unwrap_or("<unnamed>");
        let tyname = ty.name().unwrap_or("<unnamed>");
        let kind = ty.kind();
        let category = ty.parameter_category();
        let resource_shape = ty.resource_shape();
        let resource_access = ty.resource_access();
        let resource_result_type = ty.resource_result_type();
        let resource_result_type_name = resource_result_type.name().unwrap_or("<unnamed>");
        eprintln!("{name}: {tyname} (kind = {kind:?}, parameter_category = {category:?}, resource_shape = {resource_shape:?}, resource_access = {resource_access:?}, resource_result_type = {resource_result_type_name})");

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
                warn!("unimplemented parameter kind: {:?}", ty.kind());
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
    fn write_interface(&mut self, reflection: &slang::reflection::Shader) {
        // scan all parameters
        for param in reflection.parameters() {
            self.write_interface_var(param);
        }

        // scan uniforms in entry points arguments
        for entry_point in reflection.entry_points() {
            for param in entry_point.parameters() {
                self.write_interface_var(param);
            }
        }
    }
}

pub struct CtxOptions<'a> {
    pub search_paths: &'a [&'a Path],
}

impl Ctx {
    pub fn new(opts: &CtxOptions) -> Result<Self, Error> {
        let global_session = slang::GlobalSession::new().unwrap();
        let search_paths = opts
            .search_paths
            .iter()
            .map(|p| CString::new((*p).to_str().unwrap()).unwrap())
            .collect::<Vec<_>>();
        let search_path_ptrs = search_paths.iter().map(|p| p.as_ptr()).collect::<Vec<_>>();

        let session_options = slang::CompilerOptions::default()
            .optimization(slang::OptimizationLevel::None)
            .glsl_force_scalar_layout(true)
            .matrix_layout_row(true);

        let target_desc = slang::TargetDesc::default()
            .format(slang::CompileTarget::Spirv)
            .profile(global_session.find_profile("sm_6_5"));
        let targets = [target_desc];

        let session_desc = slang::SessionDesc::default()
            .targets(&targets)
            .search_paths(&search_path_ptrs)
            .options(&session_options);

        let session = //unsafe {
          // this should be unsafe
            global_session.create_session(&session_desc).expect("failed to create session");
        //};

        let mut this = Self {
            global_session,
            session,
            modules: vec![],
            visibility: syn::parse_quote!(pub),
            user_defined_types: HashMap::new(),
            counter: 0,
            output: TokenStream::new(),
            errors: Vec::new(),
            vector_type_map: HashMap::new(),
            device_ptr_type: quote!(::DeviceAddress),
        };

        // load all modules in the search paths
        for sp in opts.search_paths {
            // iterate in directory
            for entry in std::fs::read_dir(sp)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_file() {
                    let Some(ext) = path.extension() else { continue };
                    if ext == OsStr::new("slang") {
                        let module_file_name = path.file_name().unwrap().to_str().unwrap();
                        this.load_module(module_file_name).unwrap();
                    }
                }
            }
        }

        Ok(this)
    }

    fn load_module(&mut self, module_file_name: &str) -> anyhow::Result<()> {
        info!("loading module: {}", module_file_name);
        let module = match self.session.load_module(module_file_name) {
            Ok(module) => module,
            Err(err) => {
                error!("failed to load module: {:?}", err);
                return Err(anyhow!("failed to load module: {:?}", err));
            }
        };
        self.modules.push(module);
        Ok(())
    }

    pub fn reflect(&mut self) -> Result<TokenStream, Error> {
        // create composite of all modules and all their entry points
        let mut components = Vec::new();
        for module in &self.modules {
            components.push(module.downcast().clone());
            let entry_point_count = module.entry_point_count();
            for i in 0..entry_point_count {
                let entry_point = module.entry_point_by_index(i).unwrap();
                components.push(entry_point.downcast().clone());
            }
        }

        let program = self.session.create_composite_component_type(&components)?;
        let reflection = program.layout(0)?;
        self.write_interface(reflection);
        Ok(mem::take(&mut self.output))
    }
}
