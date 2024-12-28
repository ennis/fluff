use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use crate::Template;

impl Template {
    /*pub fn gen(&self) -> TokenStream {
        let mut tokens = TokenStream::new();
        self.gen_template(&mut tokens);
        tokens
    }*/

    fn gen_struct(&self) -> TokenStream {
        let element = self.elements();
        let fields = element.iter().enumerate().map(|(i, e)| {
            let name = format_ident!("_{}", i);
            let ty = format_ident!("{}", e.ty);
            quote! {
                #name: #ty
            }
        });
        quote! {
            struct __Elem {
                #(#fields),*
            }
        }
    }

    /*// generate `ElemTree` implementation
    fn gen_elem_tree_impl(&self) -> TokenStream {
        let elements = self.elements();
        let parents = vec![0; elements.len()];

        // flattened element indices
        let mut indices = Vec::new();
        for (i, e) in elements.iter().enumerate() {
            let ty = format_ident!("{}", e.ty);
            quote! {
                <#ty as #C::ElementTree>::COUNT
            }
        }
    }*/
}