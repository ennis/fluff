use std::any::{Any, TypeId};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::rc::{Rc, Weak};
use slotmap::{new_key_type, SlotMap};

/// Event sent to the global application event loop when an event is emitted by a model.
pub struct EventEmitted {
    /// The model that emitted the event.
    source: WeakAnyModel,
    /// The event payload.
    payload: Box<dyn Any>,
}

struct ModelHeader {
    type_id: TypeId,
    callbacks: Callbacks,
}

#[repr(C)]
pub struct ModelInner<T: ?Sized> {
    header: ModelHeader,
    value: RefCell<T>,
}

/// A container for a mutable piece of data that allows subscribers to listen for changes to the data.
///
/// A `Model` instance can be cloned, and moved into closures that need to listen for changes to the data.
pub struct Model<T: Any + ?Sized> {
    inner: Rc<ModelInner<T>>,
}

impl<T: Any + ?Sized> Model<T> {
    fn downgrade(&self) -> WeakModel<T> {
        WeakModel {
            inner: Rc::downgrade(&self.inner),
        }
    }

    pub fn subscribe<Event: Any>(&self, callback: impl Fn(&Event) -> bool) -> Subscription {
        /*let mut inner = self.inner.header.callbacks.
        let id = inner.header.callbacks.map.len();
        inner.header.callbacks.map.insert((inner.header.type_id, id), callback);
        Subscription(inner.header.type_id, id)*/
        todo!()
    }

    pub fn unsubscribe(&self, subscription: Subscription) {
        todo!()
    }

    pub fn update_subscription<Event: Any>(&self, subscription: Subscription, callback: impl Fn(&Event) -> bool) {
        todo!()
    }
}

impl<T: Any> Model<T> {
    pub fn as_dyn(&self) -> AnyModel {
        AnyModel {
            inner: self.inner.clone(),
        }
    }
}

pub type AnyModel = Model<dyn Any>;

pub struct WeakModel<T: Any + ?Sized> {
    inner: Weak<ModelInner<T>>,
}

impl<T: Any> WeakModel<T> {
    fn as_dyn(&self) -> WeakAnyModel {
        WeakAnyModel {
            inner: self.inner.clone(),
        }
    }
}

pub struct WeakAnyModel {
    inner: Weak<ModelInner<dyn Any>>,
}

struct PendingEvent {
    emitter: WeakAnyModel,
    payload: Box<dyn Any>,
}

struct AppContextInner {
    pending_events: Vec<PendingEvent>,
}

#[derive(Copy, Clone)]
pub struct Changed;

type Callback = Box<dyn Fn(&dyn Any) -> bool>;

struct Callbacks {
    /// Map of subscribers, indexed first by event type ID, and then by subscription index.
    map: RefCell<BTreeMap<(TypeId, usize), Callback>>,
}

impl Callbacks {
    fn push(&mut self, event_type_id: TypeId, callback: Callback) -> usize {
        let id = self.map.len();
        self.map.insert((event_type_id, id), callback);
        id
    }

    fn invoke(&mut self, event: &dyn Any) {
        // It's possible that as a result of this function being called, callbacks may be added or
        // removed.
        let tid = event.type_id();
        let mut map = self.map.take();
        let mut to_remove = Vec::new();
        for ((tid, id), callback) in map.range_mut((tid, 0)..(tid, usize::MAX)) {
            if callback(event) {
                // remove callback
                to_remove.push((*tid, *id));
            }
        }
        for (tid, id) in to_remove {
            map.remove(&(tid, id));
        }
        // merge with newly added callbacks
        self.map.borrow_mut().extend(map);
    }
}

new_key_type! {
    pub struct Subscription;
}

struct SubscriptionState {
    by_subscription: BTreeSet<(Subscription, *const Model, TypeId)>,
    by_emitter: BTreeSet<(*const Model, TypeId, Subscription)>,
    targets: SlotMap<Subscription, Callback>,
}

/*
/// A handle to a subscription made with `Model::observe`.
///
/// To unsubscribe, call `Model::unsubscribe` with this handle.
#[derive(Copy, Clone)]
pub struct Subscription(TypeId, usize);*/

////////////////////////////////////////////////////////////////////////////////////////////////////

pub struct TrackingCtx<'a> {
    tracker: &'a mut Tracker,
}

impl<'a> TrackingCtx {
    fn track(&mut self, model: AnyModel) {
        let subscription = model.subscribe::<Changed>(|_| true);
        self.tracker
            .subscriptions
            .insert(model, (subscription, self.tracker.revision));
    }
}

pub struct Tracker {
    revision: usize,
    subscriptions: BTreeMap<AnyModel, (Subscription, usize)>,
}

impl Tracker {
    pub fn with_tracking(&mut self, tracked_fn: impl FnOnce(&mut TrackingCtx), on_change: impl FnOnce()) {
        self.revision += 1;
        let mut tracking = TrackingCtx { tracker: self };

        tracked_fn(&mut tracking);

        // unsubscribe from all subscriptions that have not been renewed
        self.subscriptions.retain(|model, (subscription, revision)| {
            if *revision < self.revision {
                model.unsubscribe(*subscription);
                false
            } else {
                true
            }
        });

        for (model, (sub, _)) in self.subscriptions.iter() {
            let on_change = on_change.clone();
            model.update_subscription(*sub, move |_| {
                on_change();
                true
            });
        }
    }
}
