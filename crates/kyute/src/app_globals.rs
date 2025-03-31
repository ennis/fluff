use crate::platform::ApplicationBackend;
use std::cell::{OnceCell, RefCell};
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
    static APP_BACKEND: OnceCell<&'static ApplicationBackend> = OnceCell::new();
}

pub fn init_application() {
    APP_BACKEND.with(|g| {
        g.get_or_init(|| Box::leak(Box::new(ApplicationBackend::new())));
    });
}

pub fn teardown_application() {
    app_backend().teardown();
}

pub fn app_backend() -> &'static ApplicationBackend {
    APP_BACKEND.with(|g| *g.get().expect("an application should be active on this thread"))
}

/// Returns the system's double click time.
pub fn double_click_time() -> Duration {
    app_backend().double_click_time()
}

/// Returns the system's caret blink time.
pub fn caret_blink_time() -> Duration {
    app_backend().get_caret_blink_time()
}

/*
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

    pub fn teardown() {
        APP_GLOBALS.with(|g| g.replace(None));
    }
}
*/
