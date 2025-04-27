use crate::data::AppModel;
use fluff_gui::widgets::menu::MenuItem::{Entry, Separator, Submenu};
use fluff_gui::widgets::menu::{MenuBar, MenuEntryActivated};
use kyute::elements::{Flex, Frame};
use kyute::event::subscribe_global;
use kyute::{ElementBuilder, IntoElementAny, Window, WindowOptions};
use std::rc::Rc;
use windows::core::HSTRING;
use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_APPLMODAL, MB_ICONINFORMATION, MB_OKCANCEL, MB_TASKMODAL};
use kyute::application::spawn;

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
pub fn root_frame(app_model: Rc<AppModel>) -> impl IntoElementAny {
    use crate::ui::MainMenuEntry::*;

    // Main menu bar
    let main_menu = MenuBar::new(&[
        Submenu(
            "File",
            &[
                Entry("Load Alembic cache...", LoadAlembicCache),
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
            eprintln!("Load Alembic cache clicked");

            match id {
                LoadAlembicCache => {
                    //app_model.load_alembic_cache();

                    // display a modal here? but how? we don't have the handle to the window
                    unsafe {
                        MessageBoxW(
                            window.hwnd(),
                            &HSTRING::from("Load Alembic cache clicked"),
                            &HSTRING::from("Fluff"),
                            MB_OKCANCEL | MB_ICONINFORMATION | MB_APPLMODAL,
                        );
                    }
                    let window = window.clone();
                    spawn(async {
                        let window = Window::new(&WindowOptions {
                            modal: true,
                            owner: Some(window),
                            ..Default::default()
                        }, Frame::new());
                        window.close_requested().await;
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
