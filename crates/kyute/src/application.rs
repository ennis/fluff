use crate::debug::init_debug_state;
use crate::event::maintain_subscription_map;
use crate::platform::{run_event_loop, wake_event_loop, EventLoopWakeReason, TimerToken};
use crate::{init_application, platform, teardown_application};
use futures::executor::{LocalPool, LocalSpawner};
use futures::future::{abortable, AbortHandle};
use futures::task::LocalSpawnExt;
use slotmap::SlotMap;
use std::cell::{OnceCell, RefCell};
use std::future::Future;
use std::time::{Duration, Instant};
use tracy_client::set_thread_name;
//--------------------------------------------------------------------------------------------------

struct AppState {
    spawner: LocalSpawner,
    /// Pending callbacks.
    callbacks: RefCell<SlotMap<CallbackToken, Callback>>,
}

impl AppState {
    fn init(spawner: LocalSpawner) {
        APP_STATE.with(|state| {
            state
                .set(Self {
                    spawner,
                    callbacks: Default::default(),
                })
                .ok()
                .unwrap();
        });
    }

    fn with<R>(f: impl FnOnce(&AppState) -> R) -> R {
        APP_STATE.with(|state| f(state.get().expect("app state not initialized")))
    }
}

thread_local! {
    static APP_STATE: OnceCell<AppState> = OnceCell::new();
}

//--------------------------------------------------------------------------------------------------

slotmap::new_key_type! {
    /// A token representing a callback.
    pub struct CallbackToken;
}

impl CallbackToken {
    pub fn cancel(self) {
        cancel_callback(self);
    }
}

struct Callback {
    deadline: Option<Instant>,
    timer: Option<TimerToken>,
    func: Box<dyn FnOnce()>,
}

/// Executes pending callbacks.
///
/// Returns the time at which the next callback is scheduled to run.
pub fn run_pending_callbacks() -> Option<Instant> {
    AppState::with(|state| {
        loop {
            let now = Instant::now();
            let mut next_deadline = None;

            // Collect tasks ready to run.
            // Don't borrow `callbacks` because it may be modified by a callback.
            let ready_tasks = state
                .callbacks
                .borrow()
                .iter()
                .filter_map(|(token, callback)| {
                    if let Some(deadline) = callback.deadline {
                        if deadline <= now {
                            Some(token)
                        } else {
                            if let Some(ref mut next_deadline) = next_deadline {
                                *next_deadline = deadline.min(*next_deadline);
                            } else {
                                next_deadline = Some(deadline);
                            }
                            None
                        }
                    } else {
                        Some(token)
                    }
                })
                .collect::<Vec<_>>();

            if ready_tasks.is_empty() {
                // nothing to execute at this time
                break next_deadline;
            }

            // Run the ready tasks.
            for token in ready_tasks {
                let callback = state.callbacks.borrow_mut().remove(token).unwrap();
                (callback.func)();
            }
        }
    })
}

/// Runs a closure at a certain point in the future on the main thread.
///
/// Returns a cancellation token that can be used to cancel the timer.
fn schedule_callback(at: Option<Instant>, f: impl FnOnce() + 'static) -> CallbackToken {
    let token = AppState::with(|state| {
        let timer = if let Some(at) = at {
            // this will wake the event loop at the specified time
            Some(TimerToken::new(at))
        } else {
            None
        };

        state.callbacks.borrow_mut().insert(Callback {
            deadline: at,
            timer,
            func: Box::new(f),
        })
    });
    wake_event_loop(EventLoopWakeReason::DispatchCallbacks);
    token
}

/// Cancels a previously scheduled callback.
fn cancel_callback(token: CallbackToken) {
    AppState::with(|state| {
        let cb = state.callbacks.borrow_mut().remove(token);
        if let Some(cb) = cb {
            if let Some(timer) = cb.timer {
                // cancel the timer if there's one
                timer.cancel();
            }
        }
    });
}

/// Runs a closure at a certain point in the future on the main thread.
pub fn run_after(after: Duration, f: impl FnOnce() + 'static) -> CallbackToken {
    schedule_callback(Some(Instant::now() + after), f)
}

/// Registers a closure to run during the next iteration of the event loop, and wakes the event loop.
pub fn run_queued(f: impl FnOnce() + 'static) -> CallbackToken {
    schedule_callback(None, f)
}

/// Spawns a task on the main-thread executor.
pub fn spawn(fut: impl Future<Output = ()> + 'static) -> AbortHandle {
    AppState::with(|state| {
        let (fut, abort_handle) = abortable(fut);
        state
            .spawner
            .spawn_local(async {
                let _ = fut.await; // ignore aborts
            })
            .expect("failed to spawn task");
        abort_handle
    })
}

/// Called by the platform event loop on every event.
pub(crate) fn do_application_tick() {
    maintain_subscription_map();
}

/*
pub async fn wait_until(deadline: Instant) {
    let mut registered = false;
    poll_fn(move |cx| {
        AppState::with(|state| {
            if Instant::now() >= deadline {
                return Poll::Ready(());
            } else if !registered {
                // set waker
                let timers = &mut *state.timers.borrow_mut();
                timers.push(Timer {
                    waker: cx.waker().clone(),
                    deadline,
                });
                registered = true;
            }
            Poll::Pending
        })
    })
    .await
}

/// Waits for the specified duration.
pub async fn wait_for(duration: Duration) {
    let deadline = Instant::now() + duration;
    wait_until(deadline).await;
}*/

/// Runs the application event loop.
pub fn run(initial_future: impl Future<Output = ()> + 'static) -> Result<(), anyhow::Error> {
    set_thread_name!("UI thread");

    init_application();

    // single-threaded executor for futures
    let mut local_pool = LocalPool::new();

    // initialize main-thread-local app state
    AppState::init(local_pool.spawner());

    let true_initial_future = async move {
        // We can't call `show_debug_window` before entering the event loop,
        // because it requires the event loop to be running to create the window.
        // Thus, we call it here as part of the initial future.
        #[cfg(debug_assertions)]
        init_debug_state();
        initial_future.await;
    };

    run_event_loop(local_pool, true_initial_future)?;

    teardown_application();

    Ok(())
}

pub fn quit() {
    platform::quit();
}