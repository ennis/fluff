//! Things related to keyboard focus.

use std::cell::RefCell;
use crate::application::run_queued;
use crate::{Event, RcDynNode, WeakDynNode};
use crate::node::dispatch_event;

/// Represents the element that has the keyboard focus.
#[derive(Clone)]
pub struct FocusedElement {
    // The window in which the element is located.
    //pub window: WindowHandle,
    /// The element that has focus.
    pub element: WeakDynNode,
}

thread_local! {
    /// The element that has keyboard focus (unique among all windows).
    static FOCUSED_ELEMENT: RefCell<Option<FocusedElement>> = RefCell::new(None);

    /// The element that is capturing the pointer.
    static POINTER_CAPTURING_ELEMENT: RefCell<Option<RcDynNode>> = RefCell::new(None);
}

/// Returns the element that has keyboard focus.
pub fn get_keyboard_focus() -> Option<FocusedElement> {
    FOCUSED_ELEMENT.with(|f| f.borrow().clone())
}

/// Called to set the global keyboard focus to the specified element.
pub fn set_keyboard_focus(target: WeakDynNode) {
    run_queued(move || {
        //let parent_window = target.get_parent_window();
        let prev_focus = FOCUSED_ELEMENT.take();
        if let Some(prev_focus) = prev_focus {
            if prev_focus.element == target {
                // Element already has focus. This should be handled earlier.
                //warn!("{:?} already focused", target);
                FOCUSED_ELEMENT.replace(Some(prev_focus));
                return;
            }

            // Send FocusLost event
            if let Some(prev_focus) = prev_focus.element.upgrade() {
                prev_focus.set_focused(false);
                dispatch_event(prev_focus.clone(), &mut Event::FocusLost, false);
            }
        }

        if let Some(target) = target.upgrade() {
            // Send a FocusGained event to the newly focused element.
            target.set_focused(true);
            dispatch_event(target, &mut Event::FocusGained, false);
        }

        // Update the global focus.
        FOCUSED_ELEMENT.replace(Some(FocusedElement {
            //window: parent_window,
            element: target,
        }));

        // If necessary, activate the target window.
        //if let Some(_parent_window) = parent_window.shared.upgrade() {
        //parent_window.
        //war!("activate window")
        //}
    });
}

pub fn clear_keyboard_focus() {
    run_queued(|| {
        let prev_focus = FOCUSED_ELEMENT.take();
        if let Some(prev_focus) = prev_focus {
            if let Some(prev_focus) = prev_focus.element.upgrade() {
                prev_focus.set_focused(false);
                dispatch_event(prev_focus, &mut Event::FocusLost, false);
            }
        }
        FOCUSED_ELEMENT.replace(None);
    });
}