use crate::application::run_queued;
use color_print::cprintln;
use scoped_tls::scoped_thread_local;
use slotmap::{new_key_type, Key, KeyData, SlotMap};
use std::any::{type_name, Any, TypeId};
use std::cell::{Ref, RefCell};
use std::collections::BTreeSet;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::panic::Location;
use std::rc::{Rc, Weak};

// New
pub trait EventSource: Any {
    /// Returns this object as a weak reference.
    ///
    /// FIXME: this should not be `dyn Any`, as we never access the object, but it's the only way
    ///        to erase the type of the object.
    fn as_weak(&self) -> Weak<dyn Any>;

    #[track_caller]
    fn subscribe<E: 'static>(&self, mut callback: impl FnMut(&E) -> bool + 'static) -> SubscriptionKey
    where
        Self: Sized,
    {
        subscribe_inner(
            [self.as_weak()],
            TypeId::of::<E>(),
            Box::new(move |_source, e| {
                let e = e.downcast_ref::<E>().unwrap();
                callback(e)
            }),
            Location::caller(),
        )
    }

    #[track_caller]
    fn subscribe_once<E: 'static>(&self, callback: impl FnOnce(&E) + 'static) -> SubscriptionKey
    where
        Self: Sized,
    {
        let mut callback = Some(callback);
        self.subscribe(move |e| {
            callback.take().unwrap()(e);
            false
        })
    }

    #[track_caller]
    fn emit<E: 'static>(&self, event: E) {
        emit_inner(self.as_weak(), Box::new(event), type_name::<E>())
    }

    #[track_caller]
    async fn wait_event<E: 'static + Clone>(&self) -> E
    where
        Self: Sized,
    {
        // TODO this is not very efficient since it allocates,
        //      but it will do until I figure out how to do this with direct pointers
        //      (see the comment in `Notifier::wait`)
        //      This is especially nasty when used inside a select loop, since it will allocate
        //      a new oneshot on every iteration.
        let (tx, rx) = tokio::sync::oneshot::channel();

        let key = self.subscribe_once(move |e: &E| {
            let _ = tx.send(e.clone());
        });

        // If the future is cancelled, we need to make sure the subscription is removed
        // So wrap the subsciption in a guard object
        struct Guard(SubscriptionKey);
        impl Drop for Guard {
            fn drop(&mut self) {
                self.0.unsubscribe();
            }
        }
        let _guard = Guard(key);

        rx.await.unwrap()
    }
}

/// `Weak<dyn Any>` with pointer-based equality and ordering.
pub struct OrdWeak(pub Weak<dyn Any>);

impl Clone for OrdWeak {
    fn clone(&self) -> Self {
        Self(Weak::clone(&self.0))
    }
}

impl fmt::Debug for OrdWeak {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "WeakAny#{:08x}", Weak::as_ptr(&self.0) as *const () as usize as u32)
    }
}

impl PartialEq for OrdWeak {
    fn eq(&self, other: &Self) -> bool {
        Weak::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for OrdWeak {}

impl Ord for OrdWeak {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.0.as_ptr() as *const ()).cmp(&(other.0.as_ptr() as *const ()))
    }
}

impl PartialOrd for OrdWeak {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Hash for OrdWeak {
    fn hash<H: Hasher>(&self, state: &mut H) {
        (self.0.as_ptr() as *const ()).hash(state);
    }
}

new_key_type! {
    /// Uniquely identifies a subscription to events emitted by one or more `Model` instances.
    // TODO: subscriptions should probably be represented by guard objects, and the subscription dropped when the object is dropped
    pub struct SubscriptionKey;
}

impl SubscriptionKey {
    pub fn unsubscribe(self) {
        SUBSCRIPTION_MAP.with_borrow_mut(|s| {
            s.remove(self);
        });
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
    fn as_weak(&self) -> Weak<dyn Any> {
        let weak = Rc::downgrade(&self.inner);
        let weak: Weak<dyn Any> = weak;
        weak
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
            },
            data: RefCell::new(initial_data),
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
        track_read(self.as_weak());
        self.inner.data.borrow().clone()
    }

    /// Sets the data inside this model, and returns the previous data.
    ///
    /// Within a tracking scope, this will mark the model as both read and written to.
    #[track_caller]
    pub fn replace(&self, data: T) -> T {
        let weak = self.as_weak();
        track_read(weak.clone());
        track_write(weak);
        let old = self.inner.data.replace(data);
        self.emit(DataChanged);
        old
    }

    /// Returns a reference to the data.
    ///
    /// If this is called within a tracking scope (see `with_tracking_scope`), the model will be
    /// marked as accessed within the scope.
    pub fn borrow(&self) -> Ref<T> {
        track_read(self.as_weak());
        self.inner.data.borrow()
    }

    /// Updates the data and emits a `DataChanged` event.
    ///
    /// If this is called within a tracking scope (see `with_tracking_scope`), the model will be
    /// marked as written to within the scope.
    #[track_caller]
    pub fn update(&self, f: impl FnOnce(&mut T)) {
        track_write(self.as_weak());
        f(&mut *self.inner.data.borrow_mut());
        self.emit(DataChanged);
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
        subscribe_inner(
            [self.as_weak()],
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
        emit_inner(self.as_weak(), event, type_name::<Event>());
    }
}

/*
impl ModelAny {
    pub fn downcast<T: Any>(self) -> Option<Model<T>> {
        // FIXME: it's unfortunate that we need to borrow the RefCell here
        let type_id = (*self.inner.data.borrow()).type_id();
        if type_id == TypeId::of::<T>() {
            Some(Model {
                inner: unsafe { Rc::from_raw(Rc::into_raw(self.inner) as *const ModelInner<T>) },
            })
        } else {
            None
        }
    }
}*/

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
}

////////////////////////////////////////////////////////////////////////////////////////////////////

/// Internal object that serves as the emitter of global events.
pub struct GlobalEmitter;

impl EventSource for Rc<GlobalEmitter> {
    fn as_weak(&self) -> Weak<dyn Any> {
        Rc::downgrade(self) as Weak<dyn Any>
    }
}

thread_local! {
    /// FIXME: this doesn't support emitting events from other threads.
    static GLOBAL_EMITTER: Rc<GlobalEmitter> = Rc::new(GlobalEmitter);
}

/// Emits a global event.
///
/// Note: this should only be called on the main thread. Emitting events from other threads is not
/// currently supported.
pub fn emit_global<E: 'static>(event: E) {
    GLOBAL_EMITTER.with(|emitter| {
        let weak = Rc::downgrade(emitter);
        emit_inner(weak, Box::new(event), type_name::<E>());
    });
}

/// Subscribes to global events.
pub fn subscribe_global<E: 'static>(mut callback: impl FnMut(&E) -> bool + 'static) -> SubscriptionKey {
    GLOBAL_EMITTER.with(|emitter| {
        let weak = Rc::downgrade(emitter) as Weak<dyn Any>;
        subscribe_inner(
            [weak],
            TypeId::of::<E>(),
            Box::new(move |_source, e| {
                let e = e.downcast_ref::<E>().unwrap();
                callback(e)
            }),
            Location::caller(),
        )
    })
}

/// Asynchronously waits for a global event.
pub async fn wait_event_global<E: 'static + Clone>() -> E {
    let emitter = GLOBAL_EMITTER.with(|emitter| emitter.clone());
    emitter.wait_event().await
}

/// Watches changes on several models at once and calls a callback when any of the models change.
///
/// # Arguments
///
/// - `models`: a sequence of models to watch for changes (as weak references).
/// - `callback`: a callback that is called when any of the models change. The callback is passed
///  the model that changed, and should return whether the subscription should be kept alive.
#[track_caller]
#[inline]
pub fn watch_multi(
    models: impl IntoIterator<Item = Weak<dyn Any>>,
    callback: impl FnMut(Weak<dyn Any>) -> bool + 'static,
) -> SubscriptionKey {
    watch_multi_with_location(models, callback, Location::caller())
}

pub fn watch_multi_with_location(
    models: impl IntoIterator<Item = Weak<dyn Any>>,
    mut callback: impl FnMut(Weak<dyn Any>) -> bool + 'static,
    location: &'static Location<'static>,
) -> SubscriptionKey {
    subscribe_inner(
        models,
        TypeId::of::<DataChanged>(),
        Box::new(move |source, _e| callback(source)),
        location,
    )
}

/// Watches changes on several models at once and calls a callback _the first time_ that any of the models change.
#[track_caller]
#[inline]
pub fn watch_multi_once(
    models: impl IntoIterator<Item = Weak<dyn Any>>,
    callback: impl FnOnce(Weak<dyn Any>) + 'static,
) -> SubscriptionKey {
    watch_multi_once_with_location(models, callback, Location::caller())
}

pub fn watch_multi_once_with_location(
    models: impl IntoIterator<Item = Weak<dyn Any>>,
    callback: impl FnOnce(Weak<dyn Any>) + 'static,
    location: &'static Location<'static>,
) -> SubscriptionKey {
    let mut callback = Some(callback);
    watch_multi_with_location(
        models,
        move |source| {
            callback.take().unwrap()(source);
            false
        },
        location,
    )
}

/// Trait implemented by data model types (the `T` in `Model<T>`) that can emit events of a
/// specific type.
pub trait EventEmitter<T: Any> {}

/// Generic event emitted when a model has been changed.
///
/// This is emitted by `Model::update` or `Model::set`.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct DataChanged;

// Any type can emit a `DataChanged` event.
impl<T> EventEmitter<DataChanged> for T {}

////////////////////////////////////////////////////////////////////////////////////////////////////

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
}

////////////////////////////////////////////////////////////////////////////////////////////////////

/// `SubscriptionKey` but as a u64. It's easier to use as a key in a BTreeSet.
type SubscriptionKeyU64 = u64;

/// Closure type of subscription callbacks.
type Callback = Box<dyn FnMut(Weak<dyn Any>, &dyn Any) -> bool>;

/// Represents a subscription to an event emitted by one or more `Model` instances.
struct Subscription {
    #[cfg(debug_assertions)]
    /// Where the subscription was made, for debugging.
    location: &'static Location<'static>,
    /// The callback to invoke.
    callback: Option<Callback>,
}

impl Subscription {
    #[cfg(not(debug_assertions))]
    fn new(callback: Callback) -> Self {
        Self {
            callback: Some(callback),
        }
    }

    #[cfg(debug_assertions)]
    fn new(callback: Callback, location: &'static Location<'static>) -> Self {
        Self {
            location,
            callback: Some(callback),
        }
    }
}

/// Holds the table of subscriptions.
struct SubscriptionMap {
    /// Subscriptions ordered by source model and event type. Used when emitting events.
    by_emitter: BTreeSet<(OrdWeak, Option<TypeId>, SubscriptionKeyU64)>,
    /// Callbacks for each subscription.
    subs: SlotMap<SubscriptionKey, Subscription>,
}

fn subscribe_inner(
    sources: impl IntoIterator<Item = Weak<dyn Any>>,
    event_type_id: TypeId,
    callback: Callback,
    location: &'static Location<'static>,
) -> SubscriptionKey {
    #[cfg(debug_assertions)]
    let sub = Subscription::new(callback, location);
    #[cfg(not(debug_assertions))]
    let sub = Subscription::new(callback);

    SUBSCRIPTION_MAP.with_borrow_mut(|s| {
        let key = s.subs.insert(sub);
        for source in sources {
            s.by_emitter
                .insert((OrdWeak(source), Some(event_type_id), key.data().as_ffi()));
        }
        key
    })
}

#[track_caller]
fn emit_inner(weak_source: Weak<dyn Any>, payload: Box<dyn Any>, type_name: &str) {
    let location = Location::caller();
    let type_id = (*payload).type_id();
    let targets = SUBSCRIPTION_MAP.with_borrow_mut(|s| s.event_targets(&weak_source, type_id));
    let weak_source = OrdWeak(weak_source);

    if !targets.is_empty() {
        // TODO: why don't we queue one callback per target?
        let type_name = type_name.to_owned();

        run_queued(move || {
            #[cfg(debug_assertions)]
            {
                println!();
                let strong_count = weak_source.0.strong_count();
                let weak_count = weak_source.0.weak_count();
                cprintln!("<yellow,bold>event</>: {type_name}");
                cprintln!("   <dim>from {weak_source:?} ({strong_count} strong ref(s), {weak_count} weak ref(s))</>");
                println!("   --> {location}");
            }

            for key in targets {
                // extract the callback from the subscription while it is being called
                // to avoid locking the subscription map (callbacks may want to insert or remove
                // subscriptions)
                let cb =
                    SUBSCRIPTION_MAP.with_borrow_mut(|s| -> Option<Callback> { s.subs.get_mut(key)?.callback.take() });

                let Some(mut cb) = cb else {
                    continue;
                };

                // Print the reason why the callback is being called

                #[cfg(debug_assertions)]
                {
                    SUBSCRIPTION_MAP.with_borrow(|s| {
                        let target_location = s.subs[key].location;
                        cprintln!("   <green,bold>target</>: {key:?}");
                        println!("       --> {target_location}");
                    });
                }

                // It's possible that the model was dropped between the moment the event was emitted
                // and the moment the callback is called.
                // The event should still be sent in this case.
                //let Some(source) = weak_source.0.upgrade() else {
                //    continue;
                //};

                let keep_sub = cb(weak_source.0.clone(), &*payload);

                // put the callback back if it wasn't consumed
                SUBSCRIPTION_MAP.with_borrow_mut(|s| {
                    if !keep_sub {
                        s.subs.remove(key);
                    } else {
                        s.subs.get_mut(key).unwrap().callback = Some(cb);
                    }
                });
            }
        });
    }
}

/// Performs maintenance on the global subscription map. This should be called periodically to
/// remove expired subscriptions and dropped models.
pub(crate) fn maintain_subscription_map() {
    SUBSCRIPTION_MAP.with_borrow_mut(|s| {
        s.cleanup();
    });
}

impl SubscriptionMap {
    fn new() -> Self {
        Self {
            by_emitter: BTreeSet::new(),
            subs: SlotMap::with_key(),
        }
    }

    /// Returns the set of subscriptions interested in the event from the specified source.
    fn event_targets(&mut self, source: &Weak<dyn Any>, event_type_id: TypeId) -> Vec<SubscriptionKey> {
        // FIXME avoid cloning models
        //let weak_source = source;
        //eprintln!("looking for targets for {source:?} type_id {event_type_id:?}");
        self.by_emitter
            .range(
                (OrdWeak(source.clone()), Some(event_type_id), 0)
                    ..(OrdWeak(source.clone()), Some(event_type_id), u64::MAX),
            )
            .map(|(_, _, key)| SubscriptionKey::from(KeyData::from_ffi(*key)))
            .collect()
    }

    fn remove(&mut self, key: SubscriptionKey) {
        // Remove the subscription from the slotmap. The key will be invalidated.
        // We clean up the `by_emitter` map later when `cleanup()` is called.
        self.subs.remove(key);
    }

    /*fn remove_model(&mut self, model_ptr: *const ModelHeader) {
        self.by_emitter.retain(|k| {
            let model_id = model_ptr as usize;
            k.0 != model_id
        });

        // Alternative implementation that doesn't traverse the whole map (not sure if it's faster):
        /*let model_id = model_ptr as usize;
        let mut keys = Vec::new();
        for k in self.by_emitter.range((model_id, None, 0)..(model_id + 1, None, 0)) {
            keys.push(k);
        }
        for k in keys {
            self.by_emitter.remove(&k);
        }*/
    }*/

    /// Removes expired subscriptions and dropped models.
    fn cleanup(&mut self) {
        self.by_emitter
            .retain(|k| k.0 .0.upgrade().is_some() && self.subs.contains_key(KeyData::from_ffi(k.2).into()));
        // TODO cleanup orphan subscriptions (subscriptions without a model)
    }
}

thread_local! {
    static SUBSCRIPTION_MAP: RefCell<SubscriptionMap> = RefCell::new(SubscriptionMap::new());
}
