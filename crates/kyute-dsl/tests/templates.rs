use kyute_dsl::Template;

#[test]
fn test_simple_template() {
    let input: Template = syn::parse_quote! {
        root = <Frame> {
             background_color: #211e13;
             <TextEdit> {
                text: "Hello world";
            }
        }
    };

    let str = serde_json::to_string_pretty(&input).unwrap();
    eprintln!("{}", str);
}