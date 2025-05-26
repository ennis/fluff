use crate::data::AppModel;
use fluff_gui::widgets::dialog::{dialog_body, message_dialog, DialogButtons, DialogResult};
use fluff_gui::widgets::menu::MenuItem::{Entry, Separator, Submenu};
use fluff_gui::widgets::menu::{MenuBar, MenuEntryActivated};
use kyute::application::spawn;
use kyute::elements::{Flex, Frame};
use kyute::event::subscribe_global;
use kyute::platform::{WindowKind, WindowOptions};
use kyute::text::FontStyle::Italic;
use kyute::{text, Element, ElementBuilder, IntoElementAny, Window};
use std::rc::Rc;
use windows::core::HSTRING;
use windows::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, MB_APPLMODAL, MB_ICONINFORMATION, MB_OKCANCEL, MB_TASKMODAL,
};

mod progress_dialog;
mod timeline;
mod viewport;

#[derive(Copy, Clone)]
pub enum MainMenuEntry {
    LoadAlembicCache,
    Save,
    Exit,
}

/// The root element of the UI.
pub fn root_frame(app_model: Rc<AppModel>) -> ElementBuilder<impl Element> {
    use crate::ui::MainMenuEntry::*;

    // Main menu bar
    let main_menu = MenuBar::new(&[
        Submenu(
            "File",
            &[
                Entry("Load Alembic Cache...", LoadAlembicCache),
                Entry("Save", Save),
                Separator,
                Entry("Exit", Exit),
            ],
        ),
        //Submenu("Edit", &[Entry("Cut", 4), Entry("Copy", 5), Entry("Paste", 6)]),
    ]);

    subscribe_global::<MenuEntryActivated<MainMenuEntry>>({
        let app_model = app_model.clone();
        move |MenuEntryActivated { id, window }| {
            match id {
                LoadAlembicCache => {
                    let window = window.clone();

                    spawn(async {
                        let result = message_dialog(
                            "Load Alembic Cache",
                            text!["Loading alembic cache, please wait wait wait wait wait wait wait wait wait wait wait wait wait wait wait wait wait"],
                            DialogButtons::OK | DialogButtons::CANCEL,
                            Some(window),
                        )
                        .await;

                        match result {
                            DialogResult::Closed => {
                                eprintln!("Closed dialog");
                            }
                            DialogResult::Button(button) => {
                                eprintln!("Button clicked: {:?}", button);
                            }
                        }
                    });
                }
                Save => {
                    //app_model.save();
                }
                Exit => {
                    //app_model.exit();
                }
            }
            true
        }
    });

    // Central frame
    let central_frame = Frame::new();

    // Status bar
    let status_bar = Frame::new();

    Flex::column().child(main_menu).child(central_frame).child(status_bar)
}
