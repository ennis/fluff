use crate::application::run_queued;
use color_print::cprintln;
use slotmap::{new_key_type, Key, KeyData, SlotMap};
use std::any::{type_name, Any, TypeId};
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::hash::Hash;
use std::panic::Location;
use std::sync::{Mutex, MutexGuard, OnceLock};


new_key_type! {
    /// Uniquely identifies an event emitter.
    pub struct EmitterKey;

    /// Uniquely identifies a subscription to events emitted by one or more `Model` instances.
    // TODO: subscriptions should probably be represented by guard objects, and the subscription dropped when the object is dropped
    pub struct SubscriptionKey;
}

impl EventSource for EmitterKey {
    fn emitter_key(&self) -> EmitterKey {
        *self
    }
}

impl SubscriptionKey {
    /// Unsubscribes from the event.
    pub fn unsubscribe(self) {
        with_subscription_map(|s| s.remove_subscription(self));
    }
}

/// Trait implemented by objects that hold an emitter key and that can emit events.
pub trait EventSource: Any {
    /// Returns this object's emitter key.
    fn emitter_key(&self) -> EmitterKey;

    /// Adds a subscription to an event of the specified type emitted by this object.
    #[track_caller]
    fn subscribe<E: 'static>(&self, mut callback: impl FnMut(&E) -> bool + 'static) -> SubscriptionKey
    where
        Self: Sized,
    {
        subscribe_raw(
            [self.emitter_key()],
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
        emit_raw(self.emitter_key(), Box::new(event), type_name::<E>())
    }
}

/// Asynchronously waits for an event emitted by the specified emitter.
pub async fn wait_event<E: 'static + Clone>(emitter: EmitterKey) -> E {
    // TODO this is not very efficient since it allocates,
    //      but it will do until I figure out how to do this with direct pointers
    //      (see the comment in `Notifier::wait`)
    //      This is especially nasty when used inside a select loop, since it will allocate
    //      a new oneshot on every iteration.
    let (tx, rx) = tokio::sync::oneshot::channel();
    let mut tx = Some(tx);

    let sub_key = subscribe_raw(
        [emitter],
        TypeId::of::<E>(),
        Box::new(move |_source, e| {
            let e = e.downcast_ref::<E>().unwrap();
            let _ = tx.take().unwrap().send(e.clone());
            false
        }),
        Location::caller(),
    );

    // If the future is cancelled, we need to make sure the subscription is removed
    // So wrap the subscription in a guard object
    struct Guard(SubscriptionKey);
    impl Drop for Guard {
        fn drop(&mut self) {
            self.0.unsubscribe();
        }
    }
    let _guard = Guard(sub_key);

    rx.await.unwrap()
}

/// An owned wrapper around an `EmitterKey`. The emitter is removed when the handle is dropped.
///
/// It implements `EventSource`.
pub struct EmitterHandle(EmitterKey);

impl Default for EmitterHandle {
    fn default() -> Self {
        EmitterHandle::new()
    }
}

impl EmitterHandle {
    /// Creates a new emitter handle.
    pub fn new() -> Self {
        EmitterHandle(emitter_map().insert(()))
    }

    /// Returns the emitter key.
    pub fn key(&self) -> EmitterKey {
        self.0
    }
}

impl EventSource for EmitterHandle {
    fn emitter_key(&self) -> EmitterKey {
        self.0
    }
}

impl Drop for EmitterHandle {
    fn drop(&mut self) {
        emitter_map().remove(self.0);
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////

fn global_emitter() -> EmitterKey {
    static KEY: OnceLock<EmitterHandle> = OnceLock::new();
    KEY.get_or_init(|| EmitterHandle::new()).0
}

/// Emits a global event.
///
/// Note: this should only be called on the main thread. Emitting events from other threads is not
/// currently supported.
pub fn emit_global<E: 'static>(event: E) {
    emit_raw(global_emitter(), Box::new(event), type_name::<E>())
}

/// Subscribes to global events.
pub fn subscribe_global<E: 'static>(mut callback: impl FnMut(&E) -> bool + 'static) -> SubscriptionKey {
    subscribe_raw(
        [global_emitter()],
        TypeId::of::<E>(),
        Box::new(move |_source, e| {
            let e = e.downcast_ref::<E>().unwrap();
            callback(e)
        }),
        Location::caller(),
    )
}

/// Asynchronously waits for a global event.
pub async fn wait_event_global<E: 'static + Clone>() -> E {
    wait_event(global_emitter()).await
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
    sources: impl IntoIterator<Item = EmitterKey>,
    callback: impl FnMut(EmitterKey) -> bool + 'static,
) -> SubscriptionKey {
    watch_multi_with_location(sources, callback, Location::caller())
}

/// Same as `watch_multi`, but allows specifying the location of the call site.
pub fn watch_multi_with_location(
    sources: impl IntoIterator<Item = EmitterKey>,
    mut callback: impl FnMut(EmitterKey) -> bool + 'static,
    location: &'static Location<'static>,
) -> SubscriptionKey {
    subscribe_raw(
        sources,
        TypeId::of::<DataChanged>(),
        Box::new(move |source, _e| callback(source)),
        location,
    )
}

/// Watches changes on several models at once and calls a callback _the first time_ that any of the models change.
#[track_caller]
#[inline]
pub fn watch_multi_once(
    sources: impl IntoIterator<Item = EmitterKey>,
    callback: impl FnOnce(EmitterKey) + 'static,
) -> SubscriptionKey {
    watch_multi_once_with_location(sources, callback, Location::caller())
}

pub fn watch_multi_once_with_location(
    sources: impl IntoIterator<Item = EmitterKey>,
    callback: impl FnOnce(EmitterKey) + 'static,
    location: &'static Location<'static>,
) -> SubscriptionKey {
    let mut callback = Some(callback);
    watch_multi_with_location(
        sources,
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

/// `SubscriptionKey` but as a u64. It's easier to use as a key in a BTreeSet.
type SubscriptionKeyU64 = u64;

/// Closure type of subscription callbacks.
type Callback = Box<dyn FnMut(EmitterKey, &dyn Any) -> bool>;

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
    /// Subscriptions ordered by source and event type. Used when emitting events.
    by_emitter: BTreeSet<(EmitterKey, Option<TypeId>, SubscriptionKeyU64)>,
    /// Callbacks for each subscription.
    subs: SlotMap<SubscriptionKey, Subscription>,
}

pub fn subscribe_raw(
    sources: impl IntoIterator<Item = EmitterKey>,
    event_type_id: TypeId,
    callback: Callback,
    location: &'static Location<'static>,
) -> SubscriptionKey {
    #[cfg(debug_assertions)]
    let sub = Subscription::new(callback, location);
    #[cfg(not(debug_assertions))]
    let sub = Subscription::new(callback);

    with_subscription_map(|s| {
        let key = s.subs.insert(sub);
        for source in sources {
            s.by_emitter.insert((source, Some(event_type_id), key.data().as_ffi()));
        }
        key
    })
}

#[track_caller]
pub fn emit_raw(source: EmitterKey, payload: Box<dyn Any>, type_name: &str) {
    let location = Location::caller();
    let type_id = (*payload).type_id();
    let targets = with_subscription_map(|s| s.event_targets(source, type_id));

    if !targets.is_empty() {
        // TODO: why don't we queue one callback per target?
        let type_name = type_name.to_owned();

        run_queued(move || {
            #[cfg(debug_assertions)]
            {
                println!();
                cprintln!("<yellow,bold>event</>: {type_name}");
                cprintln!("   <dim>from {source:?}</>");
                println!("   --> {location}");
            }

            for key in targets {
                // extract the callback from the subscription while it is being called
                // to avoid locking the subscription map (callbacks may want to insert or remove
                // subscriptions)
                let Some(mut cb) = with_subscription_map(|s| {
                    let sub = s.subs.get_mut(key)?;
                    let cb = sub.callback.take()?;

                    // Print the reason why the callback is being called
                    #[cfg(debug_assertions)]
                    {
                        let target_location = s.subs[key].location;
                        cprintln!("   <green,bold>target</>: {key:?}");
                        println!("       --> {target_location}");
                    }

                    Some(cb)
                }) else {
                    continue;
                };

                // NOTE: it's possible that the model was dropped between the moment the event was emitted
                // and the moment the callback is called.
                // The event should still be sent in this case.

                // Invoke the callback
                let keep_sub = cb(source, &*payload);

                // put the callback back if it wasn't consumed
                with_subscription_map(|s| {
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
        #[cfg(debug_assertions)]
        {
            //let emitters = emitter_map();
            //eprintln!(
            //    "after cleanup: {} active subscriptions; {} live emitters",
            //    s.subs.len(),
            //    emitters.len()
            //);
        }
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
    fn event_targets(&mut self, source: EmitterKey, event_type_id: TypeId) -> Vec<SubscriptionKey> {
        // FIXME avoid cloning handles
        self.by_emitter
            .range((source, Some(event_type_id), 0)..(source, Some(event_type_id), u64::MAX))
            .map(|(_, _, key)| SubscriptionKey::from(KeyData::from_ffi(*key)))
            .collect()
    }

    fn remove_subscription(&mut self, key: SubscriptionKey) {
        // Remove the subscription from the slotmap. The key will be invalidated.
        // We clean up the `by_emitter` map later when `cleanup()` is called.
        self.subs.remove(key);
    }

    /// Removes expired subscriptions and dropped models.
    fn cleanup(&mut self) {
        let emitters = emitter_map();

        // Remove entries in the `by_emitter` map
        // - for emitters that have been removed from `emitter_map`
        // - for expired subscriptions
        self.by_emitter
            .retain(|k| emitters.contains_key(k.0) && self.subs.contains_key(KeyData::from_ffi(k.2).into()));
    }
}

thread_local! {
    static SUBSCRIPTION_MAP: RefCell<SubscriptionMap> = RefCell::new(SubscriptionMap::new());
}

/// Calls the provided function with a mutable reference to the global subscription map.
fn with_subscription_map<R>(f: impl FnOnce(&mut SubscriptionMap) -> R) -> R {
    SUBSCRIPTION_MAP.with_borrow_mut(f)
}

type EmitterMap = SlotMap<EmitterKey, ()>;

/// Locks and returns a reference to the global emitter map.
fn emitter_map() -> MutexGuard<'static, EmitterMap> {
    static EMITTER_MAP: OnceLock<Mutex<EmitterMap>> = OnceLock::new();
    EMITTER_MAP
        .get_or_init(|| Mutex::new(SlotMap::with_key()))
        .lock()
        .unwrap()
}
