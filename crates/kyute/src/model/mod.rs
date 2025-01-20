use crate::application::run_queued;
use scoped_tls::scoped_thread_local;
use slotmap::{new_key_type, Key, KeyData, SlotMap};
use std::any::{Any, TypeId};
use std::cell::{Ref, RefCell};
use std::collections::BTreeSet;
use std::hash::{Hash, Hasher};
use std::rc::{Rc, Weak};

/*
 collection models:
 - Model<Container<T>> where Container is a vec, btreemap, etc.
 - Has additional methods for manipulating the container

*/

new_key_type! {
    /// Uniquely identifies a subscription to events emitted by one or more `Model` instances.
    pub struct SubscriptionKey;
}


/// A container for a mutable piece of data that allows subscribers to listen for changes to the data.
///
/// A `Model` instance can be cloned, and moved into closures that need to listen for changes to the data.
/// Internally this is basically a `Rc<RefCell<T>>` holding a list of callbacks.
pub struct Model<T: Any + ?Sized> {
    inner: Rc<ModelInner<T>>,
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

    pub fn as_ptr(&self) -> *const ModelHeader {
        &self.inner.header as *const ModelHeader
    }
}

impl<T: Any> Model<T> {
    pub fn new(initial_value: T) -> Self {
        let inner = Rc::new(ModelInner {
            header: ModelHeader {
                type_id: TypeId::of::<T>(),
            },
            value: RefCell::new(initial_value),
        });
        Self { inner }
    }

    /// Returns a reference to the inner value.
    pub fn get(&self) -> Ref<T> {
        track_read(self.downgrade().as_dyn());
        self.inner.value.borrow()
    }

    /// Updates the value.
    pub fn update(&self, f: impl FnOnce(&mut T)) {
        track_write(self.downgrade().as_dyn());
        f(&mut *self.inner.value.borrow_mut());
    }


    pub fn as_dyn(&self) -> AnyModel {
        AnyModel {
            inner: self.inner.clone(),
        }
    }

    /// Subscribes to changes to the specified model.
    pub fn watch<Event>(&self, mut callback: impl FnMut(Model<T>, &Event) -> bool + 'static) -> SubscriptionKey
    where
        T: EventEmitter<Event>,
        Event: 'static,
    {
        let key = SUBSCRIPTION_MAP.with_borrow_mut(|s| {
            s.create_subscription(
                [self.as_dyn().downgrade()],
                TypeId::of::<Event>(),
                Box::new(move |source, payload| {
                    let event = payload.downcast_ref::<Event>().unwrap();
                    let source = source.downcast::<T>().unwrap();
                    callback(source, event)
                }),
            )
        });
        key
    }

    /// Emits an event of the specified type.
    pub fn emit<Event: 'static>(&self, event: Event)
    where
        T: EventEmitter<Event>,
    {
        let event: Box<dyn Any> = Box::new(event);
        SUBSCRIPTION_MAP.with_borrow_mut(|map| {
            map.emit(self.as_dyn(), event);
        })
    }
}

impl AnyModel {
    pub fn downcast<T: Any>(self) -> Option<Model<T>> {
        let type_id = (*self.inner.value.borrow()).type_id();
        if type_id == TypeId::of::<T>() {
            Some(Model {
                inner: unsafe { Rc::from_raw(Rc::into_raw(self.inner) as *const ModelInner<T>) },
            })
        } else {
            None
        }
    }
}

/// Creates a subscription on multiple models.
pub fn watch_multi<E: 'static>(
    models: impl IntoIterator<Item=WeakAnyModel>,
    mut callback: impl FnMut(AnyModel, &E) -> bool + 'static,
) -> SubscriptionKey {
    SUBSCRIPTION_MAP.with_borrow_mut(|s| {
        s.create_subscription(
            models,
            TypeId::of::<E>(),
            Box::new(move |source, e| {
                let e = e.downcast_ref().expect("invalid event type");
                callback(source, e)
            }),
        )
    })
}

/// Trait implemented by data model types (the `T` in `Model<T>`) that can emit events of a
/// specific type.
pub trait EventEmitter<T: Any> {}

/// Type alias for a type-erased `Model`.
pub type AnyModel = Model<dyn Any>;

/// A weak reference to a `Model` instance.
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
    pub fn upgrade(&self) -> Option<Model<T>> {
        self.inner.upgrade().map(|inner| Model { inner })
    }
}

impl<T: Any> WeakModel<T> {
    pub fn as_dyn(&self) -> WeakAnyModel {
        WeakAnyModel {
            inner: self.inner.clone(),
        }
    }
}

pub type WeakAnyModel = WeakModel<dyn Any>;

impl PartialEq for WeakAnyModel {
    fn eq(&self, other: &Self) -> bool {
        Weak::ptr_eq(&self.inner, &other.inner)
    }
}

impl Eq for WeakAnyModel {}

impl Ord for WeakAnyModel {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.inner.as_ptr() as *const ()).cmp(&(other.inner.as_ptr() as *const ()))
    }
}

impl PartialOrd for WeakAnyModel {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Hash for WeakAnyModel {
    fn hash<H: Hasher>(&self, state: &mut H) {
        (self.inner.as_ptr() as *const ()).hash(state);
    }
}

/// Generic event emitted when a model has been changed.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct ModelChanged;


////////////////////////////////////////////////////////////////////////////////////////////////////

/// Tracks accesses to models within a scope.
pub struct TrackingScope {
    /// List of models accessed (read from or written to) within the scope.
    accessed: BTreeSet<WeakAnyModel>,
    /// List of models written to within the scope.
    written: BTreeSet<WeakAnyModel>,
}

impl TrackingScope {
    pub fn new() -> Self {
        Self {
            accessed: BTreeSet::new(),
            written: BTreeSet::new(),
        }
    }

    /// Adds a subscription to the accessed models.
    pub fn watch_once<E, F>(&self, callback: F) -> SubscriptionKey
    where
        E: 'static,
        F: FnOnce(AnyModel, &E) -> bool + 'static,
    {
        let mut callback = Some(callback);
        watch_multi(self.accessed.iter().cloned(), move |source, event| {
            let callback = callback.take().unwrap();
            callback(source, event);
            false
        })
    }
}

scoped_thread_local!(static TRACKING_SCOPE: RefCell<TrackingScope>);

pub fn with_tracking_scope<F, R>(f: F) -> (R, TrackingScope)
where
    F: FnOnce() -> R,
{
    let mut tracking_scope = RefCell::new(TrackingScope::new());
    let r = TRACKING_SCOPE.set(&tracking_scope, f);
    (r, tracking_scope.into_inner())
}

pub(crate) fn track_read(model: WeakAnyModel) {
    if TRACKING_SCOPE.is_set() {
        TRACKING_SCOPE.with(move |s| {
            s.borrow_mut().accessed.insert(model);
        });
    }
}

pub(crate) fn track_write(model: WeakAnyModel) {
    if TRACKING_SCOPE.is_set() {
        TRACKING_SCOPE.with(move |s| {
            s.borrow_mut().accessed.insert(model.clone());
            s.borrow_mut().written.insert(model);
        });
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////

/// Internals of a `Model` instance.
#[repr(C)]
struct ModelInner<T: ?Sized> {
    header: ModelHeader,
    value: RefCell<T>,
}

// TODO: this is useless since we carry around type information as `dyn Any`. This is a relic
// of trying to make a thin pointer to a type-erased `Model` instance.
struct ModelHeader {
    type_id: TypeId,
}

/// `SubscriptionKey` but as a u64. It's easier to use as a key in a BTreeSet.
type SubscriptionKeyU64 = u64;

/// Closure type of subscription callbacks.
type Callback = Box<dyn FnMut(AnyModel, &dyn Any) -> bool>;

/// Represents a subscription to an event emitted by one or more `Model` instances.
struct Subscription {
    callback: Option<Callback>,
}

/// Holds the table of subscriptions.
struct SubscriptionMap {
    /// Subscriptions ordered by source model and event type. Used when emitting events.
    by_emitter: BTreeSet<(WeakAnyModel, Option<TypeId>, SubscriptionKeyU64)>,
    /// Callbacks for each subscription.
    subs: SlotMap<SubscriptionKey, Subscription>,
}

impl SubscriptionMap {
    fn new() -> Self {
        Self {
            by_emitter: BTreeSet::new(),
            subs: SlotMap::with_key(),
        }
    }

    /// Returns the set of subscriptions interested in the event from the specified source.
    fn event_targets(&mut self, source: &AnyModel, event_type_id: TypeId) -> Vec<SubscriptionKey> {
        // FIXME avoid downgrade / cloning models
        let weak_source = source.downgrade();
        self.by_emitter
            .range((weak_source.clone(), Some(event_type_id), 0)..(weak_source.clone(), Some(event_type_id), u64::MAX))
            .map(|(_, _, key)| SubscriptionKey::from(KeyData::from_ffi(*key)))
            .collect()
    }

    fn emit(&mut self, source: AnyModel, payload: Box<dyn Any>) {
        let type_id = payload.type_id();
        let targets = self.event_targets(&source, type_id);
        let weak_source = source.downgrade();

        if !targets.is_empty() {
            run_queued(move || {
                for key in targets {
                    // extract the callback from the subscription while it is being called
                    // to avoid locking the subscription map (callbacks may want to insert or remove
                    // subscriptions)
                    let cb = SUBSCRIPTION_MAP
                        .with_borrow_mut(|s| -> Option<Callback> { s.subs.get_mut(key)?.callback.take() });

                    let Some(mut cb) = cb else {
                        continue;
                    };

                    // It's possible that the model was dropped between the moment the event was emitted
                    // and the moment the callback is called.
                    let Some(source) = weak_source.upgrade() else {
                        continue;
                    };

                    let keep_sub = cb(source, &*payload);

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

    fn create_subscription(
        &mut self,
        sources: impl IntoIterator<Item=WeakAnyModel>,
        event_type_id: TypeId,
        callback: Callback,
    ) -> SubscriptionKey {
        let key = self.subs.insert(Subscription {
            callback: Some(callback),
        });
        for source in sources {
            self.by_emitter
                .insert((source, Some(event_type_id), key.data().as_ffi()));
        }
        key
    }

    fn remove_subscription(&mut self, key: SubscriptionKey) {
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
            .retain(|k| k.0.upgrade().is_some() && self.subs.contains_key(KeyData::from_ffi(k.2).into()));
    }
}

thread_local! {
    static SUBSCRIPTION_MAP: RefCell<SubscriptionMap> = RefCell::new(SubscriptionMap::new());
}
