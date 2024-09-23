use crate::backend::ApplicationBackend;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

//==================================================================================================

/// Application globals.
///
/// Stuff that would be too complicated/impractical/ugly to carry and pass around as parameters.
pub struct AppGlobals {
    pub(crate) backend: ApplicationBackend,
}

thread_local! {
    static APP_GLOBALS: RefCell<Option<Rc<AppGlobals>>> = RefCell::new(None);
}

impl AppGlobals {
    /// Creates a new `Application` instance.
    pub fn new() -> Rc<AppGlobals> {
        // TODO: make sure that we're not making multiple applications
        let backend = ApplicationBackend::new();
        let app = Rc::new(AppGlobals { backend });

        APP_GLOBALS.with(|g| g.replace(Some(app.clone())));
        app
    }

    pub fn try_get() -> Option<Rc<AppGlobals>> {
        APP_GLOBALS.with(|g| Some(g.borrow().as_ref()?.clone()))
    }

    pub fn get() -> Rc<AppGlobals> {
        AppGlobals::try_get().expect("an application should be active on this thread")
    }

    pub fn double_click_time(&self) -> Duration {
        self.backend.double_click_time()
    }

    pub fn teardown() {
        APP_GLOBALS.with(|g| g.replace(None));
    }
}
