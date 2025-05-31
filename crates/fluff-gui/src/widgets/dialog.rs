use crate::widgets::button::Button;
use crate::widgets::TEXT_STYLE;
use bitflags::bitflags;
use kyute::elements::{ClickedEvent, Flex, Frame, Text};
use kyute::event::{wait_event, EmitterKey};
use kyute::layout::SizeValue;
use kyute::platform::{PlatformWindowHandle, WindowKind, WindowOptions};
use kyute::text::IntoTextLayout;
use kyute::{select, Color, Element, NodeBuilder, EventSource, Window};

bitflags! {
    /// Flags for the dialog buttons.
    #[derive(Copy, Clone, Default, Debug)]
    pub struct DialogButtons: u32 {
        /// OK.
        const OK = 0x1;
        /// Yes.
        const YES = 0x4;
        /// No.
        const NO = 0x8;
        /// Retry.
        const RETRY = 0x10;
        /// Save.
        const SAVE = 0x20;
        /// Ignore.
        const IGNORE = 0x40;
        /// Close without saving.
        const CLOSE_WITHOUT_SAVING = 0x100;
        /// Cancel.
        const CANCEL = 0x2;
        /// Apply.
        const APPLY = 0x80;
    }
}

#[derive(Copy, Clone)]
pub enum DialogResult {
    Closed,
    Button(DialogButtons),
}

fn dialog_buttons_inner(buttons: DialogButtons, emitter: EmitterKey) -> NodeBuilder<impl Element> {
    let mut frame = Frame::new().padding(8.);
    let mut hbox = Flex::row().gap(3.).initial_gap(SizeValue::Stretch);

    let mut insert_button = |button: DialogButtons, label: &str| {
        if buttons.contains(button) {
            hbox.add_child(Button::new(label).on::<ClickedEvent>(move |_this, _cx, _event| {
                emitter.emit(button);
            }));
        }
    };

    insert_button(DialogButtons::OK, "OK");
    insert_button(DialogButtons::YES, "Yes");
    insert_button(DialogButtons::NO, "No");
    insert_button(DialogButtons::RETRY, "Retry");
    insert_button(DialogButtons::SAVE, "Save");
    insert_button(DialogButtons::IGNORE, "Ignore");
    insert_button(DialogButtons::CLOSE_WITHOUT_SAVING, "Close Without Saving");
    insert_button(DialogButtons::CANCEL, "Cancel");
    insert_button(DialogButtons::APPLY, "Apply");

    frame = frame.content(hbox).background_color(Color::from_hex("#323232"));
    frame
}

pub fn dialog_body(message: impl IntoTextLayout, buttons: DialogButtons) -> NodeBuilder<impl Element> {
    let mut vbox = Flex::column().gap(SizeValue::Stretch);
    vbox.add_child(
        Frame::new()
            .content(Text::new(message.into_text_layout(TEXT_STYLE)))
            .padding_top(20.0)
            .padding_bottom(20.0)
            .padding_left(20.0),
    );
    vbox.add_child(dialog_buttons_inner(buttons, vbox.emitter_key()));
    vbox
}

/// Spawns a standard message dialog with the given message and buttons.
///
/// Returns the button that was pressed.
pub async fn message_dialog(
    title: &str,
    message: impl IntoTextLayout,
    buttons: DialogButtons,
    modal_owner_window: Option<PlatformWindowHandle>,
) -> DialogResult {
    let body = dialog_body(message, buttons);
    let emitter = body.emitter_key();

    let window = Window::new(
        &WindowOptions {
            title,
            kind: WindowKind::Modal(modal_owner_window),
            resizable: false,
            center: true,
            ..Default::default()
        },
        body,
    );

    // wait for window closed or button pressed
    select! {
        _ = window.close_requested() => {
            DialogResult::Closed
        }
        button = wait_event::<DialogButtons>(emitter) => {
            DialogResult::Button(button)
        }
    }
}
