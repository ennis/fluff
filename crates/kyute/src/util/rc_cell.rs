use rc_borrow::RcBorrow;
use std::cell::{Cell, UnsafeCell};
use std::ops::{Deref, DerefMut};
use std::rc::{Rc, UniqueRc, Weak};
use thiserror::Error;

type BorrowFlag = isize;
const UNUSED: BorrowFlag = 0;

#[inline(always)]
fn is_writing(x: BorrowFlag) -> bool {
    x < UNUSED
}

#[inline(always)]
fn is_reading(x: BorrowFlag) -> bool {
    x > UNUSED
}

#[derive(Error, Debug, Copy, Clone)]
#[error("already mutably borrowed")]
pub struct BorrowError;

#[derive(Error, Debug, Copy, Clone)]
#[error("already borrowed")]
pub struct BorrowMutError;

/// Like `RefCell`, but borrowed via `Rc` instead of simple references.
pub struct RcCell<T: ?Sized> {
    borrow: Cell<BorrowFlag>,
    inner: UnsafeCell<T>,
}

impl<T> RcCell<T> {
    pub fn new(inner: T) -> Self {
        Self {
            borrow: Cell::new(UNUSED),
            inner: UnsafeCell::new(inner),
        }
    }
}

impl<T: ?Sized> RcCell<T> {
    pub fn try_lock_read(&self) -> bool {
        let b = self.borrow.get().wrapping_add(1);
        // Taken from the implementation of RefCell::try_borrow in the standard library
        if !is_reading(b) {
            // Overflow or write lock
            false
        } else {
            self.borrow.set(b);
            true
        }
    }

    pub unsafe fn unlock_read(&self) {
        let borrow = self.borrow.get();
        debug_assert!(is_reading(borrow));
        self.borrow.set(borrow - 1);
    }

    pub fn try_lock_write(&self) -> bool {
        match self.borrow.get() {
            UNUSED => {
                self.borrow.set(UNUSED - 1);
                true
            }
            _ => false,
        }
    }

    pub unsafe fn unlock_write(&self) {
        let borrow = self.borrow.get();
        debug_assert!(is_writing(borrow));
        self.borrow.set(borrow + 1);
    }

    pub fn try_borrow(self: &Rc<Self>) -> Result<RcRef<T>, BorrowError> {
        if self.try_lock_read() {
            Ok(RcRef {
                inner: RcBorrow::from(self),
            })
        } else {
            Err(BorrowError)
        }
    }

    pub fn try_borrow_mut(self: &Rc<Self>) -> Result<RcRefMut<T>, BorrowMutError> {
        if self.try_lock_write() {
            Ok(RcRefMut {
                inner: RcBorrow::from(self),
            })
        } else {
            Err(BorrowMutError)
        }
    }

    pub fn borrow(self: &Rc<Self>) -> RcRef<T> {
        self.try_borrow().expect("already mutably borrowed")
    }

    pub fn borrow_mut(self: &Rc<Self>) -> RcRefMut<T> {
        self.try_borrow_mut().expect("already borrowed")
    }

    pub fn get(&self) -> *mut T {
        self.inner.get()
    }

    pub unsafe fn try_borrow_unguarded(self: &Rc<Self>) -> Result<&T, BorrowError> {
        // Adapted from RefCell::try_borrow_unguarded in std
        if !is_writing(self.borrow.get()) {
            Ok(unsafe { &*self.inner.get() })
        } else {
            Err(BorrowError)
        }
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////

pub struct RcRef<'a, T: ?Sized> {
    inner: RcBorrow<'a, RcCell<T>>,
}

impl<T: ?Sized> RcRef<'_, T> {
    pub fn downgrade(self) -> Weak<RcCell<T>> {
        RcBorrow::to_weak(self.inner)
    }
}

impl<T: ?Sized> Drop for RcRef<'_, T> {
    fn drop(&mut self) {
        // SAFETY: the only way to get a RcCellMut is to lock the borrow flag
        // by calling try_lock_write
        unsafe { self.inner.unlock_read() }
    }
}

impl<T: ?Sized> Deref for RcRef<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // SAFETY: TODO
        unsafe { &*self.inner.inner.get() }
    }
}

////////////////////////////////////
pub struct RcRefMut<'a, T: ?Sized> {
    inner: RcBorrow<'a, RcCell<T>>,
}

impl<T: ?Sized> RcRefMut<'_, T> {
    pub fn downgrade(self) -> Weak<RcCell<T>> {
        RcBorrow::to_weak(self.inner)
    }
}

impl<T: ?Sized> Drop for RcRefMut<'_, T> {
    fn drop(&mut self) {
        // SAFETY: the only way to get a RcCellMut is to lock the borrow flag
        // by calling try_lock_write
        unsafe { self.inner.unlock_write() }
    }
}

impl<T: ?Sized> Deref for RcRefMut<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // SAFETY: RcCellMut locks the borrow flag
        unsafe { &*self.inner.inner.get() }
    }
}

impl<T: ?Sized> DerefMut for RcRefMut<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // SAFETY: RcCellMut locks the borrow flag
        unsafe { &mut *self.inner.inner.get() }
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////

/*

// Like Rc<RefCell<T>>, but can return `RefMut`s that are `RcBorrows` and can be directly
// downgraded to Weak<RefCell<T>>
pub struct RcCell<T: ?Sized>(Rc<Inner<T>>);

impl<T: ?Sized> Clone for RcCell<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: ?Sized> RcCell<T> {
    pub fn try_borrow(&self) -> Result<RcCellRef<T>, BorrowError> {
        if self.0.try_lock_read() {
            Ok(RcCellRef {
                inner: RcBorrow::from(&self.0),
            })
        } else {
            Err(BorrowError)
        }
    }

    pub fn try_borrow_mut(&self) -> Result<RcCellRefMut<T>, BorrowMutError> {
        if self.0.try_lock_write() {
            Ok(RcCellRefMut {
                inner: RcBorrow::from(&self.0),
            })
        } else {
            Err(BorrowMutError)
        }
    }

}

pub struct WeakCell<T: ?Sized> {
    inner: Weak<Inner<T>>,
}

impl<T: ?Sized> WeakCell<T> {
    pub fn upgrade(&self) -> Option<RcCell<T>> {
        self.inner.upgrade().map(RcCell)
    }
}

impl<T: ?Sized> Clone for WeakCell<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}*/

////////////////////////////////////////////////////////////////////////////////////////////////////

pub struct UniqueRcCell<T: ?Sized>(UniqueRc<RcCell<T>>);

impl<T> UniqueRcCell<T> {
    pub fn new(value: T) -> Self {
        Self(UniqueRc::new(RcCell::new(value)))
    }
}

impl<T: ?Sized> UniqueRcCell<T> {
    pub fn weak(&self) -> WeakCell<T> {
        WeakCell {
            inner: UniqueRc::downgrade(&self.0),
        }
    }
}

impl<T: ?Sized> Deref for UniqueRcCell<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // SAFETY: UniqueRc ensures that the `Inner` struct is unique, so no sharing is possible
        // outside of borrowing from this `UniqueRcCell` itself
        unsafe { &*self.0.inner.get() }
    }
}

impl<T: ?Sized> DerefMut for UniqueRcCell<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.as_mut().inner.get_mut()
    }
}
