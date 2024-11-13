use kyute_dsl::Control;

#[test]
fn test_control_decl() {
    let input: Control = syn::parse_quote! {

        pub control TestControl;

        #[attribute]
        pub property test: f32;
        pub property test2: f32;

        event text_changed_by_user();
        event selection_changed_by_user();
        event editing_finished();

        Frame [root] {
             TextEdit {
                text: "Hello world",
                text2: {
                    self.get_text()
                }

                text_changed_by_user() => {
                    self.set_text(text);
                    self.text_changed_by_user();
                }
            }
        }
    };

    dbg!(input);
}