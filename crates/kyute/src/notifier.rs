use slab::Slab;
use std::cell::{RefCell, UnsafeCell};
use std::future::poll_fn;
use std::task::{Poll, Waker};

/// Holds a list of callbacks, taking a value of type `T`, that can be invoked or waited on.
#[derive(Default)]
pub struct Notifier<T> {
    cb: RefCell<Slab<Box<dyn Fn(T)>>>,
}

impl<T: 'static> Notifier<T> {
    /// Creates a new instance.
    pub fn new() -> Self {
        Self {
            cb: RefCell::new(Slab::new()),
        }
    }

    /// Adds a callback to the list.
    ///
    /// Returns an ID that can be used to remove the callback.
    pub fn watch(&self, cb: impl Fn(T) + 'static) -> usize {
        self.cb.borrow_mut().insert(Box::new(cb))
    }

    /// Removes a callback from the list.
    ///
    /// # Arguments
    /// * `id` - The ID of the callback to remove, that was returned by a corresponding call to `watch`.
    pub fn unwatch(&self, id: usize) {
        let _ = self.cb.borrow_mut().remove(id);
    }

    /// Asynchronously waits for the event to be notified.
    pub async fn wait(&self) -> T {
        let mut result = UnsafeCell::new(None);
        let mut waker = UnsafeCell::new(None);

        let result_ptr = result.get();
        let waker_ptr: *mut Option<Waker> = waker.get();

        let cb_index = self.watch(move |t| {
            unsafe {
                // SAFETY: no other references to `result` or `waker` are live
                *result_ptr = Some(t);
                if let Some(waker) = (*waker_ptr).take() {
                    waker.wake();
                }
            }
        });

        poll_fn(|cx| {
            // safety: `waker` is only accessed from the callback and not at the same time because
            // Callbacks is !Sync so all references to it are within the same thread.
            unsafe {
                *waker_ptr = Some(cx.waker().clone());
                if (*result_ptr).is_some() {
                    Poll::Ready(())
                } else {
                    Poll::Pending
                }
            }
        }).await;

        self.unwatch(cb_index);
        unsafe { (*result.get()).take().unwrap() }
    }
}

impl<T: Clone + 'static> Notifier<T> {
    /// Invokes all callbacks with the specified value.
    pub fn invoke(&self, t: T) {
        // FIXME: can't add callbacks while iterating
        for (_, cb) in self.cb.borrow().iter() {
            cb(t.clone());
        }
    }
}