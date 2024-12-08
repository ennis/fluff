extern crate proc_macro;


use kyute_dsl::Template;
use proc_macro::TokenStream;


#[proc_macro]
pub fn control(input: TokenStream) -> TokenStream {
    let control = syn::parse_macro_input!(input as Template);

    todo!()
}