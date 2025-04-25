use crate::event::{
    emit_raw, subscribe_raw, DataChanged, EmitterHandle, EmitterKey, EventEmitter, SubscriptionKey,
};
use crate::EventSource;
use std::any::{type_name, Any, TypeId};
use std::cell::{Ref, RefCell, RefMut};
use std::fmt;
use std::hash::{Hash, Hasher};
use std::ops::{Deref, DerefMut};
use std::panic::Location;
use std::rc::{Rc, Weak};

/// A writable reference to the data of a model.
///
/// This is akin to `RefMut` of `RefCell`.
pub struct ModelMut<'a, T> {
    rm: RefMut<'a, T>,
}

impl<'a, T> Deref for ModelMut<'a, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &*self.rm
    }
}

impl<'a, T> DerefMut for ModelMut<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut *self.rm
    }
}

pub struct ModelRef<'a, T> {
    r: Ref<'a, T>,
}

impl<'a, T> Deref for ModelRef<'a, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &*self.r
    }
}

/// A container for a mutable piece of data that allows subscribers to listen for changes to the data.
///
/// `Model` instances have reference semantics similar to `Rc`. They can be cheaply cloned, and clones
/// refer to the same underlying data. The weak reference counterpart is [`WeakModel`].
pub struct Model<T: Any + ?Sized> {
    inner: Rc<ModelInner<T>>,
}

impl<T: Any> EventSource for Model<T> {
    fn emitter_key(&self) -> EmitterKey {
        self.inner.header.emitter.key()
    }
}

impl<T: Any + ?Sized> Clone for Model<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<T: Any + ?Sized> Model<T> {
    pub fn downgrade(&self) -> WeakModel<T> {
        WeakModel {
            inner: Rc::downgrade(&self.inner),
        }
    }
}

impl<T: Any> Model<T> {
    /// Creates a new model with the specified initial data.
    pub fn new(initial_data: T) -> Self {
        let inner = Rc::new(ModelInner {
            header: ModelHeader {
                _type_id: TypeId::of::<T>(),
                emitter: EmitterHandle::new(),
            },
            data: RefCell::new(initial_data),
        });
        Self { inner }
    }

    pub fn new_cyclic<F>(f: F) -> Self
    where
        F: FnOnce(WeakModel<T>) -> T,
    {
        let inner = Rc::new_cyclic(|weak| ModelInner {
            header: ModelHeader {
                _type_id: TypeId::of::<T>(),
                emitter: EmitterHandle::new(),
            },
            data: RefCell::new(f(WeakModel { inner: weak.clone() })),
        });
        Self { inner }
    }

    /// Returns a clone of the data inside this model.
    ///
    /// Within a tracking scope, this will mark the model as accessed.
    pub fn get(&self) -> T
    where
        T: Clone,
    {
        // track_read(self.as_weak());
        self.inner.data.borrow().clone()
    }

    /// Returns a writable reference to the data.
    pub fn write(&self) -> ModelMut<T> {
        // TODO figure out whether we want to keep implicit tracking
        //track_write(self.as_weak());
        ModelMut {
            rm: self.inner.data.borrow_mut(),
        }
    }

    /// Returns a reference to the data.
    pub fn read(&self) -> ModelRef<T> {
        // TODO figure out whether we want to keep implicit tracking
        //track_read(self.as_weak());
        ModelRef {
            r: self.inner.data.borrow(),
        }
    }

    /// Sets the data inside this model, and returns the previous data.
    ///
    /// Within a tracking scope, this will mark the model as both read and written to.
    #[track_caller]
    pub fn replace(&self, data: T) -> T {
        //let weak = self.as_weak();
        //track_read(weak.clone());
        //track_write(weak);
        let old = self.inner.data.replace(data);
        self.emit(DataChanged);
        old
    }

    /// Returns a reference to the data.
    ///
    /// If this is called within a tracking scope (see `with_tracking_scope`), the model will be
    /// marked as accessed within the scope.
    pub fn borrow(&self) -> Ref<T> {
        //track_read(self.as_weak());
        self.inner.data.borrow()
    }

    /// Updates the data and emits a `DataChanged` event.
    ///
    /// If this is called within a tracking scope (see `with_tracking_scope`), the model will be
    /// marked as written to within the scope.
    #[track_caller]
    pub fn update(&self, f: impl FnOnce(&mut T)) {
        //track_write(self.as_weak());
        f(&mut *self.inner.data.borrow_mut());
        self.emit(DataChanged);
    }

    /// Updates the data.
    #[track_caller]
    pub fn modify(&self, f: impl FnOnce(&mut T, WeakModel<T>)) {
        f(&mut *self.inner.data.borrow_mut(), self.downgrade());
    }

    /// Returns a type-erased reference to the model.
    pub fn as_dyn(&self) -> ModelAny {
        ModelAny {
            inner: self.inner.clone(),
        }
    }

    /// Watches changes to the model data (i.e. a `DataChanged` event) and calls the callback when the data changes.
    ///
    /// # Return value
    ///
    /// A `SubscriptionKey` identifying the resulting subscription to the model, that can be used
    /// to remove the subscription later.
    #[track_caller]
    pub fn watch(&self, mut callback: impl FnMut() -> bool + 'static) -> SubscriptionKey {
        subscribe_raw(
            [self.inner.header.emitter.key()],
            TypeId::of::<DataChanged>(),
            Box::new(move |_source, _e| callback()),
            Location::caller(),
        )
    }

    /// Emits an event of the specified type.
    #[track_caller]
    pub fn emit<Event: 'static>(&self, event: Event)
    where
        T: EventEmitter<Event>,
    {
        let event: Box<dyn Any> = Box::new(event);
        emit_raw(self.inner.header.emitter.key(), event, type_name::<Event>());
    }
}

/// Type alias for a type-erased `Model`, i.e. `Model<dyn Any>`.
pub type ModelAny = Model<dyn Any>;

impl fmt::Debug for ModelAny {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "ModelAny#{:08x}",
            Rc::as_ptr(&self.inner) as *const () as usize as u32
        )
    }
}

/// A weak reference to a `Model` instance, obtained with `Model::downgrade`.
pub struct WeakModel<T: Any + ?Sized> {
    inner: Weak<ModelInner<T>>,
}

impl<T: Any + ?Sized> Clone for WeakModel<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<T: Any + ?Sized> WeakModel<T> {
    /// Tries to upgrade this weak reference to a strong reference to the model data.
    ///
    /// Returns `None` if the model has been dropped.
    pub fn upgrade(&self) -> Option<Model<T>> {
        self.inner.upgrade().map(|inner| Model { inner })
    }
}

impl<T: Any> WeakModel<T> {
    /// Returns a type-erased weak reference to the model.
    pub fn as_dyn(&self) -> WeakModelAny {
        WeakModelAny {
            inner: self.inner.clone(),
        }
    }
}

/// Type-erased weak reference to a `Model` instance.
pub type WeakModelAny = WeakModel<dyn Any>;

impl fmt::Debug for WeakModelAny {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "WeakModelAny#{:08x}",
            Weak::as_ptr(&self.inner) as *const () as usize as u32
        )
    }
}

impl PartialEq for WeakModelAny {
    fn eq(&self, other: &Self) -> bool {
        Weak::ptr_eq(&self.inner, &other.inner)
    }
}

impl Eq for WeakModelAny {}

impl Ord for WeakModelAny {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.inner.as_ptr() as *const ()).cmp(&(other.inner.as_ptr() as *const ()))
    }
}

impl PartialOrd for WeakModelAny {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Hash for WeakModelAny {
    fn hash<H: Hasher>(&self, state: &mut H) {
        (self.inner.as_ptr() as *const ()).hash(state);
    }
}

/// Internals of a `Model` instance.
#[repr(C)]
struct ModelInner<T: ?Sized> {
    header: ModelHeader,
    data: RefCell<T>,
}

// TODO: this is useless since we carry around type information as `dyn Any`. This is a relic
// of trying to make a thin pointer to a type-erased `Model` instance.
struct ModelHeader {
    _type_id: TypeId,
    emitter: EmitterHandle,
}

////////////////////////////////////////////////////////////////////////////////////////////////////

/*
/// Tracks accesses to models within a scope.
pub struct TrackingScope {
    /// List of models accessed (read from or written to) within the scope.
    /// TODO: WeakAny set
    pub reads: BTreeSet<OrdWeak>,
    /// List of models written to within the scope.
    pub writes: BTreeSet<OrdWeak>,
}

impl TrackingScope {
    pub fn new() -> Self {
        Self {
            reads: BTreeSet::new(),
            writes: BTreeSet::new(),
        }
    }

    /// Adds a subscription to the accessed models.
    pub fn watch_once<F>(&self, callback: F) -> SubscriptionKey
    where
        F: FnOnce(Weak<dyn Any>) -> bool + 'static,
    {
        let mut callback = Some(callback);
        watch_multi(self.reads.iter().map(|w| w.0.clone()), move |source| {
            let callback = callback.take().unwrap();
            callback(source);
            false
        })
    }
}

scoped_thread_local!(static TRACKING_SCOPE: RefCell<TrackingScope>);

pub fn with_tracking_scope<F, R>(f: F) -> (R, TrackingScope)
where
    F: FnOnce() -> R,
{
    let tracking_scope = RefCell::new(TrackingScope::new());
    let r = TRACKING_SCOPE.set(&tracking_scope, f);
    (r, tracking_scope.into_inner())
}

/// Registers a read access to the specified model within the current tracking scope.
pub(crate) fn track_read(model: Weak<dyn Any>) {
    if TRACKING_SCOPE.is_set() {
        TRACKING_SCOPE.with(move |s| {
            s.borrow_mut().reads.insert(OrdWeak(model));
        });
    }
}

/// Registers a write access to the specified model within the current tracking scope.
pub(crate) fn track_write(model: Weak<dyn Any>) {
    if TRACKING_SCOPE.is_set() {
        TRACKING_SCOPE.with(move |s| {
            s.borrow_mut().reads.insert(OrdWeak(model.clone()));
            s.borrow_mut().writes.insert(OrdWeak(model));
        });
    }
}*/
