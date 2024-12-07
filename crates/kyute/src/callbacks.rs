use slab::Slab;
use std::cell::{RefCell, UnsafeCell};
use std::future::poll_fn;
use std::task::{Poll, Waker};

#[derive(Default)]
pub struct Callbacks<T> {
    cb: RefCell<Slab<Box<dyn Fn(T)>>>,
}

impl<T: 'static> Callbacks<T> {
    pub fn new() -> Self {
        Self {
            cb: RefCell::new(Slab::new()),
        }
    }

    pub fn watch(&self, cb: impl Fn(T) + 'static) -> usize {
        self.cb.borrow_mut().insert(Box::new(cb))
    }

    pub fn unwatch(&self, id: usize) {
        let _ = self.cb.borrow_mut().remove(id);
    }

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

impl<T: Clone + 'static> Callbacks<T> {
    pub fn invoke(&self, t: T) {
        // FIXME: can't add callbacks while iterating
        for (_, cb) in self.cb.borrow().iter() {
            cb(t.clone());
        }
    }
}