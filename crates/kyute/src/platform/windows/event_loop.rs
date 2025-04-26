//! Win32 event loop implementation.
//!
//! For now we use winit, but we might want to replace that with our own solution.

use crate::application::{do_application_tick, run_pending_callbacks, spawn};
use crate::event::maintain_subscription_map;
use crate::platform::windows::window::{handle_window_event, redraw_windows};
use crate::platform::EventLoopWakeReason;
use crate::{app_backend, AbortHandle};
use anyhow::Context;
use futures::executor::LocalPool;
use scoped_tls::scoped_thread_local;
use slotmap::SlotMap;
use std::cell::RefCell;
use std::future::Future;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::{LazyLock, Mutex, OnceLock};
use std::time::{Duration, Instant};
use winit::event::{Event, StartCause};
use winit::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy, EventLoopWindowTarget};

//------------------------------------------------------------------------------------------------

scoped_thread_local! {
    /// winit needs a reference to an EventLoopWindowTarget to create a window. It's too annoying to
    /// pass it around, so we use a thread-local variable to store it.
    static EVENT_LOOP_WINDOW_TARGET: EventLoopWindowTarget<EventLoopWakeReason>
}

/// Pending timers.
static TIMERS: LazyLock<Mutex<SlotMap<TimerToken, Timer>>> = LazyLock::new(|| Mutex::new(SlotMap::with_key()));

static PENDING_REDRAW: AtomicBool = AtomicBool::new(false);

/// Proxy to wake the event loop from other threads.
static EVENT_LOOP_PROXY: OnceLock<EventLoopProxy<EventLoopWakeReason>> = OnceLock::new();

//------------------------------------------------------------------------------------------------

/// Accesses the current "event loop window target", which is used to create winit [winit::window::Window]s.
pub(crate) fn with_event_loop_window_target<T>(f: impl FnOnce(&EventLoopWindowTarget<EventLoopWakeReason>) -> T) -> T {
    EVENT_LOOP_WINDOW_TARGET.with(|event_loop| f(&event_loop))
}

slotmap::new_key_type! {
    /// Uniquely identifies a timer.
    pub struct TimerToken;
}

impl TimerToken {
    pub fn new(at: Instant) -> Self {
        TIMERS.lock().unwrap().insert(Timer { deadline: at })
    }

    pub fn cancel(self) {
        TIMERS.lock().unwrap().remove(self);
    }
}

/// Pending timer information.
struct Timer {
    deadline: Instant,
}

/// Wakes the event loop on the main thread.
///
/// # Arguments
///
/// * `reason` - The reason for waking the event loop.
pub fn wake_event_loop(reason: EventLoopWakeReason) {
    if reason == EventLoopWakeReason::CompositorClockTick {
        // don't queue multiple compositor clock ticks
        if PENDING_REDRAW.swap(true, Relaxed) {
            return;
        }
    }
    EVENT_LOOP_PROXY.get().unwrap().send_event(reason).unwrap()
}

/// Enters the application event loop (i.e. runs the application).
///
/// # Arguments
///
pub fn run_event_loop(
    mut executor: LocalPool,
    initial_future: impl Future<Output = ()> + 'static,
) -> Result<(), anyhow::Error> {
    // check for run_event_loop being called multiple times
    if EVENT_LOOP_PROXY.get().is_some() {
        panic!("run_event_loop called multiple times");
    }

    // create the event loop and its proxy
    let event_loop: winit::event_loop::EventLoop<EventLoopWakeReason> = EventLoopBuilder::with_user_event()
        .build()
        .context("failed to create the event loop")?;
    EVENT_LOOP_PROXY.set(event_loop.create_proxy()).unwrap();

    // Spawn and poll the initial future, so that windows are created.
    // This is necessary because if no windows are created no messages will be sent and
    // the closure passed to `run` will never be called.
    EVENT_LOOP_WINDOW_TARGET.set(&event_loop, || {
        spawn(initial_future);
        executor.run_until_stalled();
    });

    // start the compositor clock; this will periodically wake the event loop with
    // CompositorClockTick events
    app_backend().start_compositor_clock();

    let mut next_render_start_time = Instant::now();

    // Enter the event loop
    event_loop.run(move |event, elwt| {
        EVENT_LOOP_WINDOW_TARGET.set(elwt, || {
            //let event_time = Instant::now().duration_since(event_loop_start_time);
            match event {
                // TIMERS //////////////////////////////////////////////////////////////////
                Event::NewEvents(cause) => {
                    match cause {
                        StartCause::ResumeTimeReached { .. } | StartCause::WaitCancelled { .. } | StartCause::Poll => {
                            run_pending_callbacks();

                            // see if we need to redraw
                            if PENDING_REDRAW.load(Relaxed) {
                                // if the next render start time is in the past, we need to redraw
                                if next_render_start_time <= Instant::now() {
                                    // REDRAW //////////////////////////////////////////////
                                    redraw_windows();
                                    // accept new redraw requests
                                    PENDING_REDRAW.store(false, Relaxed);
                                }
                            }
                        }
                        StartCause::Init => {}
                    }
                }

                // USER WAKEUP /////////////////////////////////////////////////////////////
                Event::UserEvent(EventLoopWakeReason::DispatchCallbacks) => {
                    run_pending_callbacks();
                }

                // COMPOSITOR WAKEUP ///////////////////////////////////////////////////////
                Event::UserEvent(EventLoopWakeReason::CompositorClockTick) => {
                    // VBlank interrupt received. We now have a deadline of one vblank
                    // interval to render the next frame.
                    //
                    // To minimize input latency, we should keep receiving input events and
                    // begin redraw at the last possible moment, ensuring that the time
                    // it takes to render the frame won't exceed the deadline.
                    //
                    // Thus, the optimal time for starting the redraw is
                    // NEXT_VBLANK_TIME - RENDER_TIME where RENDER_TIME is the time it takes
                    // to render a frame. We can't know RENDER_TIME in advance,
                    // but we can estimate it based on previous frames.
                    //
                    // Note that these calculations depend on the event loop receiving the
                    // CompositorClockTick event on time, without delay.

                    // TODO: actually measure the time it takes to render a frame
                    let input_delay = Duration::from_millis(10);

                    // wait a bit before redrawing to allow for more input events,
                    // then redraw everything
                    next_render_start_time = Instant::now() + input_delay;
                    let _ = TimerToken::new(next_render_start_time);
                }

                // WINDOW EVENTS ///////////////////////////////////////////////////////////
                Event::WindowEvent {
                    window_id,
                    event: window_event,
                } => {
                    handle_window_event(window_id, window_event);
                }
                _ => {}
            };

            // do one application tick
            do_application_tick();

            // run tasks that were possibly unblocked as a result of propagating events
            executor.run_until_stalled();

            // compute next timer deadline
            let next_deadline = {
                let timers = TIMERS.lock().unwrap();
                timers.iter().map(|(_, timer)| timer.deadline).min()
            };

            // set control flow to wait until next timer expires, or wait until next
            // event if there are no timers
            match next_deadline {
                Some(deadline) => elwt.set_control_flow(ControlFlow::WaitUntil(deadline)),
                None => elwt.set_control_flow(ControlFlow::Wait),
            };
        });
    })?;

    Ok(())
}

/// Terminates the event loop.
pub fn quit() {
    with_event_loop_window_target(|event_loop| {
        event_loop.exit();
    });
}
