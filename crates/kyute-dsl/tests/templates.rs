use kyute_dsl::Template;

#[test]
fn test_simple_template() {
    let input: Template = syn::parse_quote! {
        root = <Frame> {
             direction: "vertical";
             padding: 4;
             background_color: #211e13;
             initial_gap: 1fr;
             final_gap: 1fr;

             <TextEdit> {
                text: "Hello world";
            }
        }
    };

    let button: Template = syn::parse_quote! {
        <Frame> {
            direction: "horizontal";
            padding: 4px;
            width: max(100px, min_content);
            height: max(40px, min_content);
            initial_gap: 1fr;
            final_gap: 1fr;

            text = <Text> {}
        }
    };

    let str = serde_json::to_string_pretty(&input).unwrap();
    eprintln!("{}", str);
}