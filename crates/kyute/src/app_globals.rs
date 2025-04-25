use crate::platform::ApplicationBackend;
use std::cell::OnceCell;
use std::time::Duration;

//==================================================================================================

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

