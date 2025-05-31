# Should there be a cascade system for styles?

Maybe not worth the complexity.
Stick with a basic immutable map of styles.

# Element tree V2?

We allocate a lot (lots of Rc's), and there are many different kinds of references involved:

- `&Element`: ref to element base state, the most common, can be used to control / query stuff about the element, can be
  upgraded to an `Rc<dyn ElementMethods>`
- `&dyn ElementMethods`: ref to an element and its methods (event, layout, etc.), derefs to `&Element`
- `Rc<dyn ElementMethods>`: strong refcounted ptr to the above, derefs to `&Element`
- `Weak<dyn ElementMethods>`: weak refcounted ptr to the above
- `Rc<T> where T: ElementMethods` return type of T::new()

What we want to keep: the ability to call methods on `Element` from the derived element type `T` (currently made
possible by the fact that `T` holds an `Element`).

Compare with GC languages, where you have two types, `Element` for the base class and `T` for the derived object.

Can we do without ref-counting? Might be complicated.
Alternative: allocate elements in a pool?

Can we store `&Element`? Of course not, would need to add a lifetime to every struct that holds it, and the lifetime
isn't clear in the first place.

## Concretely

An element is a combination of its base fields and the derived fields. It can be represented in rust like this:

```rust 
// Derived-owns-base
struct Derived {
    base: Base,
}

// Base-owns-derived (unsized)
struct Base<T: ?Sized> {
    // base fields...
    derived: T
}

impl Derived {
    fn test(&self) {
        // how to access base fields?
    }
}

```

The first approach seems nicer because methods on `Derived` are automatically aware of the base fields, whereas in the
second approach the base fields must be passed as a separate parameter.

## Alternative design

- With arbitrary_self_types, methods on `T` get `self: &Element<Self>`, i.e. it's inside-out (the Element owns the
  derived type).
- Refer to nodes by IDs (possibly strongly-typed)
    - ~~There's only one tree in the whole application anyway~~, and it should be accessible only on the main thread.
    - There's one tree per window, but all trees could share the same arena
        - Issue: accessing a tree node would require locking the arena, extremely easy to lock across an await point,
          which is bad
        - Issue: even a simple read needs locking
        - Solution: IDs that are really pointers into the slab, but hold a strong ref (like Rc)
        - Not sure if this is any more efficient than just Rc
    - Pass a reference to the tree everywhere
      -> No thanks; the syntactical noise and additional ceremony to do simple tree manipulation doesn't justify the (
      possibly marginal) perf gains - I probably won't measure them anyway; plus elements have variable size, so we
      can't
      pool them efficiently
      -> Concretely: every function on derived elements will need a pointer to the tree do modify anything that requests
      a repaint or relayout
      -> That's an additional `tree: &Tree` pointer on basically everything that accesses the tree; or even `&mut Tree`
      for mutations, god forbid, which would require a complete rearchitecture to work correctly.

Generally, it's not a goal to remove RefCells and such. It might feel "clean" and "idiomatic" but then you end up doing
nonsense like referencing nodes by ID or even ID paths (lenses), with additional indirections,
instead of the obvious thing, which is storing a pointer to the element that you want to reference.

## Issues

- `Rc<dyn ElementMethods>` is big? Would prefer `Rc<Element>`, but lose access to the methods
- Read-only tree traversals require refcount increment/decrements: can't return `&Element`, at the very least we can
  return `Ref<Element>` but that's annoying.
    - Hopefully the compiler can elide the increment/decrement in simple cases, since it's not atomic

## Conclusion

The only thing that could potentially improve the current architecture is using a (deterministic, single-threaded)
tracing garbage collector for the element tree. Possibly `gc-arena`, although the generative `'gc` lifetime feels scary,
and the need to pass a `&Mutation<'gc>` around kinda defeats the purpose.

# Styling: open or closed property sets?

I.e. `Vec<PropertyDeclaration>` or `TypeMap<StyleValue>`?
Might be useful to keep the set open for custom layout stuff, like dock panels, etc.

Decision: don't put anything related to style in `Element`, let individual widgets handle styling themselves. If
some kind of CSS is necessary, implement it on top of the element tree.
However, keep the "AttachedProperty" hack for now to support custom layouts. Maybe make it so that it doesn't have to
allocate for small things (limit the number of types that it can store?).

# Grid layout

CSS grid layout is somewhat complicated.
Either:

1. use taffy and get CSS layout for free
2. implement a greatly simplified grid layout

Possible grid layout simplifications:

- no autoflow
- no sizing to contents
- no baseline calculations

-> this really cripples what we can do with it

-> use taffy; it won't be tied to the Element tree anyway. If somehow at some point we want to make a UI (for a game?)
that doesn't need taffy or that needs a simpler layout/styling model for performance, we don't have to pay for it
(just don't use frames).

# Intuitive layout system

* Element = boxes. A box is represented as a combination of a size + the offset in the container coordinate space.
* Elements determine their size depending on available space passed down from containers, and the parent container size,
  if it is known
* Parent can request sizes from their children in h or v axes separately, and under different sizing modes
* There are different sizing modes available, inspired from CSS box sizing
    * min-content: minimum size so that the content isn't clipped (typical example: min-content size of a paragraph is
      the size of its longest word)
    * max-content: size given infinite available space (as if the available space was infinite)
    * definite: there is definite available space
* Sizes can be defined as a percentage of the parent container size
    * When measuring, if the parent container size is not known, then it uses the max-content size
* Elements can overflow their parent container. This should be handled by the container.
* Usually the parent container positions the element itself, but the child element can return positioning
  info (an explicit vec2 offset in the coord space of the container, relative to the content box)
* The parent container size isn't necessarily equal to the available size

For positioning, the child element could also return information:

- alignment
- spacing relative to prev/next element? spacing along main axis
- spacing along cross axis (before/after)

- top/left/right/bottom: Sizing
    - if top == Sizing::Flex, then it acts as a spring in that dimension

~~# Alternative: linear system of equations
Each box has~~
-> don't do this, too limited, no min/max in eqns, no inequality constraints, text layout becomes complicated

## Box positioning and alignment

5 reference lines: top, bottom, left, right, and baseline
left/right: spacing between the left/right edge of the container (or the right/left edge of the previous/next item) and
the left/right edge of this box

Issue: centering? There's no reference line for the horizontal and vertical centers

For positioning, only one of top/bottom/baseline should be set to determine the position of the box.

However, for items in column flex containers, both top and bottom can be specified at the same time to set the spacing
before and after the element. Also, baseline alignment is meaningless in that context, as is the horizontal center line.
-> it quickly devolves into a non-linear system of constraints which isn't super intuitive

* For centering, use 1x stretch on each side of the axis to center

# Component model

## Proposal A: async

Three objects:

1. the element in the UI tree: holds an abort handle to the task
2. the async task: waits for changes in the model, or events emitted by the elements, as an alternative to callbacks;
   emits signals
3. the model object: holds property values, events, methods to get/set/watch changes and emit events.

When creating a component, the user gets two things: the model object which can be used to communicate and control the
component (communicate with the async task) and the element, to place in the UI tree.

Dependencies between values are explicit: i.e. there is always code somewhere that watches for changes in the value
and then re-runs some code when it changes.

There are no callbacks; in place of callbacks, there are async tasks that are scheduled to run when the value of a model
changes. Unsure whether this is better than callbacks.

Notifications emitted by the model include the index of the property that changed and information about which items
of the property changed if it is a collection.

Issues:
The main problem with this approach is communicating with the async task from the component above. It's no longer
a simple function call.

## Proposal B: callbacks

One object: the element/component in the UI tree

The component holds a list of callbacks to invoke when properties change. A callback is composed of a weak pointer
to a receiver, and a function pointer. The weak pointer is upgraded when the callback is invoked.

Issues: the user must be careful not to smuggle a Rc inside a callback that would create a reference cycle.
Also, all callbacks are invoked immediately (unless a queueing mechanism is added).

## Proposal C: async II

Two (one-and-a-half) objects:

- the element/component in the async tree
- the async task, owned by the component and self-referential

Methods can be called directly on the component if it doesn't need to communicate with the async task.
The async task gets a reference (not a strong ref) to the component. It's only there to handle events.
It's safe to store a ref to the component in the async task because it is pinned, and no mut refs are possible.
When the component is dropped, the abort handle is dropped as well, which cancels the task before it has a chance of
observing an invalid component.
There's some subtlety to figure out around destructors inside the async task, since they may reference the component.

# Alternative to trait ElementMethods: enum

```rust
use std::marker::PhantomData;

enum ElementKind {
    Frame(FrameElement),
    Text(TextElement),
    Component {
        component: Rc<dyn Component>,
        task_abort_handle: AbortHandle,
    },
    Custom {
        // handles layout, painting, etc.
        delegate: Rc<dyn ElementDelegate>
    }
}

struct FrameElement {
    children: RefCell<Element>,
}

struct Element {
    weak_this: Weak<Element>,
    kind: ElementKind,
}

impl Element {
    pub fn frame() -> Rc<Element> {
        // ...
    }

    pub fn children(&self) -> Ref<[Rc<Element>]> {
        // ...
    }

    pub fn add_child(&self, child: &Element) {}
}

trait Component {
    fn root(&self) -> &Element;
}

// Issue: `Element`s lose the component type
// Solution: custom pointer type

struct ComponentPtr<T> {
    elem: Rc<Element>,
    _phantom: PhantomData<Rc<T>>,
}

impl<T> ComponentPtr<T> {
    pub fn new(component: T) -> ComponentPtr<T> {}
}

// Issue: this doesn't deref to Element, so this can't be used in `add_child`
impl<T> Deref for ComponentPtr<T> {
    type Target = T;
}

```

Q: can we do cheaper than Rc?

Observation: most elements live as long as the async scope in which they are instantiated.
Thus, there is a well-defined lifetime for those (the scope of the async block).
Subtrees could almost refer to their children by simple borrows, which would be cool.
However, we would have no way of holding a pointer to an element outside the
async scope in which they are declared: i.e. no `WeakElement` to hold the currently focused element, no parent links,
etc.

Alternative: NodeIds, and a global thread_local arena of Elements.
Limitation: no way of borrowing stuff from this arena without `Ref` wrappers or closure-based APIs (yuck).

```rust
struct FrameEl<'a> {
    children: RefCell<Vec<&'a Element>>,
}

struct Element<'a> {
    // parent links are essential to propagate dirty flags since we don't have 
    parent: UnsafeCell<*const>
}

// BAD IDEA
```

Proposal: instead of requiring `element()` in the `Element` impl, use `Element<T>`,
and something like:

```rust
pub struct Widget {
    // ...    
}

impl Widget {
    pub fn new() -> Element<Widget> {
        Element::new(|weak| {
            Widget {
                // can hold the weak ptr here if necessary
            }
        })
    }

    pub fn set_something(&self) {
        // if necessary, use WeakElement to invalidate ourselves
        // It's a bit inefficient since `set_something` will almost always be called from `Element<Widget>`
        self.weak.mark_needs_layout();
    }
}

```

Issue: `Element<T> -> Element<dyn Element>` coercion is atrocious.

Current approach: emulate inheritance by storing `Element` inside `Derived`, then impl `Deref<Target=Element>`

- type erasure: `Derived` unsizes to `dyn ElementMethods`
- access to derived methods: `Derived::new` returns `Rc<Derived>` so it's good
- access to element methods inside the derived widget: methods on element are directly accessible since the derived owns
  the element base

Alternate approach: store the derived element inside `Element<Derived>`, then impl `Deref<Target=Derived>`

- type erasure: `Element<Derived>` should unsize to `Element<dyn ElementMethods>` but that's not automatic yet,
  functions should take `impl IntoElement`
- access to derived methods: via Deref impl
- access to element methods inside the derived widget: derived object should hold a weak pointer to itself

Alternate-alternate approach: elements has an `ElementKind` enum, custom elements in a separate trait

- type erasure: `Element` is the basic object and has no type parameter to erase; functions take `impl IntoElement`
- access to derived methods: must use a custom wrapper over `Element` that derefs to the custom element

What's wrong with the current types?

Another alternative: require all elements to implement the `add_child`, etc. methods.
These can be no-ops for elements without children.
However we'd need to implement all this for every element type:

- parent link
- child list
- transform/geometry
- name
- attached properties

Conclusion: stay with current design, avoid using `&Element` (now `&Node`) directly, prefer `&dyn ElementMethods`
(now `&dyn Element`).

## Optimization opportunity

Most subtrees are static. Instead of storing all elements in a `Vec<RcElement>`, use a generic `ElementTree`, that is
implemented by arrays,
vectors, etc.
Reference elements with `(Rc<dyn ElementTree> + index)`. This avoids many allocations.
The root object is an `Rc<dyn ElementTree>` instance.
Element index `-1` is the root of the tree, `0..len` are the descendant elements. The descendant elements represent a
flattened hierarchy, not a flat list of direct child elements.

An `ElementTree` is composed of `ElementNode`s, each node is either:

- a static node, holding the index of this node in the parent tree, and the range of children
- a dynamic subtree node, holding a `Rc<ElementTree>`

# Usefulness of async VS callbacks?

In most cases, async could be replaced by callbacks.

Futures+async:

* (+) ability to react to multiple element events while still having direct mutable access to local state
* (+) no closures, so no need to clone stuff so that they can be captured
* (-) this forces the code to be far away from where the element is declared

Callbacks:

* (-) moving stuff into closures is atrocious

Relevant HN comment:

> A classic use case for coroutines is easily turning a push (callback) interface into a pull (return) interface. I'm
> sure many people have thought of using coroutines to handle GUI events this way. (...)
> However, GUI frameworks are a nightmare of criss-crossing events and state, and in practice callbacks are the least of
> your worries. In other domains callback interfaces often force you to scatter what would otherwise be highly localized
> logic, but it's the nature of GUI frameworks that your event logic will tend to be short, chunked, and scattered
> regardless, which is demonstrated by the toy examples. Yes, the async pattern turned event pushes into event pulls
> syntactically, but that's it--there was no real payoff.


Q: are state machines in GUI common and complex enough to benefit from an async approach?
A: not sure, except for gestures, but that's not really complex

The only remaining benefit is having mutable access for local state. However, if it somehow needs to be read by the
parent, then state will need to be stored in the component's fields, with Cell or RefCell.

# Mismatch between top-level windows and widgets

The lifetime of top-level windows is tied to the scope in which they are created. They become visible when created
and are destroyed when the object is dropped.

# Proposals:

- (Maybe) Split structure from behavior
    - the proc-macro should only build the element tree, binding constant properties and names to child elements, but
      shouldn't contain any logic, including reactive bindings

- (Guiding principle) live reloading. True hot-reloading implies that changes to hot-reloadable portions cannot change
  what the rust side can observe at compile-time. In turn, this means that **no rust code is generated at build time**.
- And in turn, all queries from rust code to live components will be done at runtime.
- As an alternative, if statically-typed properties and events are needed, they should be created in a rust object that
  is then exposed to the live side via bindings.

Summary:
On the rust side, define a `MyComponent` struct containing all needed properties, events and behavior, implemented in
pure rust. These are exposed to the live side via automatically-generated bindings.
On the live side, define the content of the component.

```rust 

//------------------------------------------------
// 1st part: define the struct

// `component` attribute automatically registers this type for use in live-reloaded component macros.
#[component]
struct Spinner {
    node: Node,
    suffix: String,
    value: Cell<f64>,
    value_changed: Handler,
}

//------------------------------------------------
// 2nd part: define the structure (body) of the component.
// This can be hot-reloaded.
// This contains no logic and no code.

component! {
    // impl ComponentBody for Spinner
    Spinner => {
        Frame {
            TextEdit[text_edit]
            
            // This is also a component
            Button[inc] {
                label: "+ Increment";
            }
    
            Button[dec] {
                label: "- Decrement";
            }
        }
    }
}

//------------------------------------------------
// 3rd part: behavior

impl Spinner {
    fn update_text(&self) {
        let text_edit = ui!(text_edit);
        text_edit.set_text(format!("{}{}", self.value, self.suffix));
    }

    pub fn value(&self) -> f64 {
        self.value.get()
    }
}

impl Component for Spinner {
    async fn task(&self) {
        // retrieve references to UI elements
        let inc = ui!(inc);
        let dec = ui!(dec);

        loop {
            // wait for events
            select! {
                // watch clicks on `inc` and `dec`, update value as necessary.
            }
        }
    }
}


```

# Pain points

- Having to split component implementation and task handle in two allocations is suboptimal
- Communicating with the async task from the parent component is unwieldy

# Idea: `Rc<Components>` are themselves futures

```rust 
use std::cell::RefCell;
use std::future::Future;

struct Component {
    future: RefCell<F>,  // impl Future
}

impl Future for Component {
    type Output = ();

    // Required method
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // poll the inner future; the inner future holds a reference to 
        // the component itself, which is OK.
        //
        // Poll is called when the event loop is woken up with a specific component reference.
        // I.e. when the event loop is woken up, we know it's because of a specific component, and thus we can poll
        // its future immediately.


        // NO: still not OK, during `Drop`, destructors inside the future state may observe a partially deleted
        // component.
        // We must make sure that the future is dropped **before** the component, but since the component **owns**
        // the future, all components must have a custom drop impl. 
        // OR: we could make sure that the future state is always the first field of the component so that it is deleted first...
        // but it's very sketchy.
    }
}

```

# Callback version

```rust
#[component]
struct Spinner {
    node: Node,
    suffix: String,
    value: Cell<f64>,
    value_changed: Handler,
}

impl Spinner {
    fn new() -> Rc<Spinner> {
        let inc = button("+");  // Rc<Button>
        let dec = button("-");


        Node::new_derived(|node| Spinner {})
    }

    fn update_text(&self) {
        let text_edit = ui!(text_edit);
        text_edit.set_text(format!("{}{}", self.value, self.suffix));
    }

    pub fn value(&self) -> f64 {
        self.value.get()
    }
}

impl Element for Spinner {}

```

# Two approaches to GUI

The "declarative" approach (react-like): when state changes, **rebuild** the UI tree (or at least part of it).
Local state is tricky, needs a **reconciliation** procedure with a retained tree.

Imperative mutation approach: build the initial tree, then mutate it when state changes.

# Nice-to-haves

## Describing widget trees: native Rust struct syntax

Describe widget trees with simple value types, built with struct initialization syntax.
Will be made more ergonomic with [default field values](https://github.com/rust-lang/rfcs/pull/3681).
Almost certainly implies a separation between the retained widget tree and a transient "view" tree rebuilt on every
frame, because nodes of the retained widget tree are somewhat hard to create directly (they need parent pointers).

Avoid builder patterns. It's pure boilerplate for the widget developer.

## Direct references to elements in the UI tree (DIRECT-WIDGET-REFS)

Previously elements were directly owned by their parents. It was impossible to keep a borrow to an element for long
(the lifetimes would be intractable), so elements had a unique ID, and could be reached by ID paths from the root to the
element.

This ended up complicating the implementation of the widget trait (widgets needed to "collaborate" to deliver events to
the proper child widget), so don't do that again. Instead, it should be possible to directly refer to a widget by a
reference, most likely `Rc/Weak` pointers.

## Reactivity VS explicit tree mutation (REACTIVITY)

In short, reactivity means: instead of explicitly mutating the tree in response to events, track the values that the
UI tree depends on when it is first built; then when those value changes, rebuild the tree.
Those "values" are typically associated to a parent scope. Usage of values are tracked in the scope. Usually when a
value changes, the scope and all its parents are re-evaluated.

E.g. ```VBox { Text { text: format!("{value}") } }``` will create an implicit dependency on `value`. The whole
expression
is rebuilt when `value` changes.

Reactive systems are interesting because you don't have to write separate code for initialization and update of the
UI tree. However, they can be more difficult to implement and harder to follow (notably, dependencies between state and
UI are largely implicit).

They also tend to be coarser-grained: since dependent values are tracked at the level of
UI elements and not individual properties, whole elements are rebuilt even if just one property actually changes.
Frameworks tend to solve that by having a separate retained tree, onto which the rebuilt UI tree is applied by
diffing individual property values.

This adds a bit of complexity, and actually requires a whole other system for the retained tree. A lot of reactive
systems gloss over that fact since they manipulate the DOM underneath.

Reactivity is important: if you have a piece of data that changes, and several UI elements that show this data,
users shouldn't have to keep track of all dependent UI elements and update them manually when the data changes.

### Counter-approach: explicit tree mutation

A counter-approach to reactivity is explicit modification of the UI tree in callbacks, like so:

```
impl Component {
    fn new(value: Model<u32>) {
        let vbox = ...;
        let text = ...;
        
        value.on_changed(|this, value| {
            this.text.set_value(format!("{value}"));
        });
    }
}
```

This requires references to elements in the retained UI tree.
Something like this is impossible in reactive frameworks, in which the widget tree is (mostly) immutable.
In reactive frameworks, if some property of a widget needs to change, it needs to be recreated.

Note that it's possible to provide a declarative syntax similar to reactive systems, but that desugars to callbacks
and explicit tree mutations.

### Pull vs push

Another difference of this approach is that it is more "push-based" than reactive systems: the retained tree is eagerly
updated on every mutation. This can potentially result in redundant work, if many mutations occur on the same part of
the tree.

In contrast, reactive systems are more "pull-based": a changing reactive value marks dependent scopes for rebuild, which
occurs once all changes are processed. This can avoid redundant work on the retained tree.

It's not clear which of pull OR push is the best. Pull would be more suited when a lot of changes occur between frames,
whereas push may be more optimized for localized data changes.

### In the wild

Nearly all rust-native GUI libraries adopt a reactive pattern. This is probably encouraged by the language which
is somewhat hostile to long-lived references to inner values.
Explicit tree mutation is largely unexplored (or avoided?)

See also [this comment](https://www.reddit.com/r/rust/comments/e04b1p/towards_a_unified_theory_of_reactive_ui/f8cpw8u/)

> > I do not see a real difference between the reactive code example and the object oriented example except for the
> > syntax. The first is declarative, the second is imperative.


> I have a slide on this in my upcoming talk. From a Rust perspective, the first can be written pretty cleanly in
> current druid, but the second requires the object references to be Rc<RefCell> (or, better, weak pointers for the
> callbacks). That's unpleasant not just because of ergonomics, but because it creates opportunities for runtime
> failure,
> either panic or deadlock, as callback chains become more complex.

* unpleasant because of ergonomics: yes
* opportunities for runtime failure: with callback chains, that's true, but async might be able to clean things up.
    * For instance, with async and scoped callbacks, moving weak pointers in callbacks is not necessary anymore.
    * Interior mutability is probably inevitable, unless we can keep the state inside a future.

## Context-less widget methods. (NO-CONTEXT)

Methods of widgets (for instance: `TextInput::move_cursor`, `Checkbox::is_checked`, `Button::set_focus`) shouldn't need
a `Ctx` parameter to work correctly. We should be able to call those methods directly, in any context, given
a reference to the widget (e.g. `text_input.move_cursor_to_next_word()` instead of
`text_input.move_cursor_to_next_word(ctx)`).

Rationale: in a previous design, callback-taking methods needed a `Ctx` instance. This is pure syntactical noise for
the user of the library. We should strive to eliminate any "accidental concepts" that don't bring any control to the
user of the library.

A practical consequence of that is that it should be possible to obtain a shared reference to a widget from `&self`.
(So that it's possible to use methods that store strong references to widgets).

## No complicated wrapper types and non-standard receiver types. (NO-CUSTOM-RECEIVERS)

Avoid methods with signatures like `fn method(self: &Rc<Self>)`. This is surprising, unergonomic. Prefer storing
a cyclic weak pointer inside the object so that it can be upgraded to Rc when needed.

## Minimize the need for `Rc` wrapping in user code (NO-USER-RC)

If possible, avoid the need for wrapping in Rc for custom widgets: it should be `Widget::new() -> Widget`, not
`Widget::new() -> Rc<Widget>`. If it's necessary, wrap with `Rc` in `add_child`.
Why? Minimize noise when building deep UI trees.

## Hot-reloading (HOT-RELOAD)

There are several flavors of hot-reloading:

- hot-reloading of static property values: doable, already done in the old kyute, but quickly becomes limited
- hot-reloading element tree structure: reload the element tree and property bindings, but can't create new events, or
  properties. Significantly more useful, and enforces a separation between presentation (hot-reloadable)
  and behavior (static, in rust), when designing the element tree DSL, and for the consumer of the library.
- full-flavor: hot-reload element trees, and also hot-reload behavior. This means that code in components are written in
  some sort of scripting language (or that components are written in rust but compiled to WASM modules or some
  complication like that)

Hot-reloading is most likely incompatible with specifying widget trees in pure rust, unless we compile rust code at
runtime (possibly to WASM modules).

## Avoid unnecessary allocations (NO-ALLOC)

For instance, a vbox with a fixed number of items shouldn't allocate. It should be generic over the subtree type
(in static cases, a tuple).

## No "virtual DOM" (NO-VDOM)

Avoid virtual DOM approaches that require reconciliation. The retained tree should be directly, and efficiently mutable.

## Separation of presentation and behavior (PRESENTATION-VS-BEHAVIOR)

This is not required per-se, but if hot-reload is a goal it will be easier to do if presentation and behavior are
separate.

## Async event handling (ASYNC-EVENTS)

Respond to events by awaiting futures. Multiple events can be awaited by `select!`ing multiple futures. In contrast
to callbacks, it's possible to keep mutable state in the local scope of the future, instead of having to wrap it in
`Rc<RefCell>`, and cloning them for each callback.

There are challenges with async event handling, the most significant is managing the lifetime of the task spawned to
handle widget events. It needs to be tied to a widget, but it's easy to accidentally create a reference cycle this way,
by moving a strong ref to the element into the async task that manages it (a common pattern). Weak pointers can
circumvent this issue, but it's easy to accidentally leak memory by keeping an upgraded weak pointer across
an await point. Languages with cycle-collecting GCs don't have this issue.

Examples in the wild:

- async_ui: not only events are futures that are awaited, you also need to `await` elements themselves for them to be
  rendered. In my opinion this is too unintuitive and error-prone.
  The library itself targets HTML elements and thus papers over a lot of complexity. Doesn't seem very popular.
- https://notgull.net/async-gui/: only advocacy, no implementation yet
- a (very) old incarnation of kyute, back when it was miniqt
    - since it was Qt under the hood, a lot of complexity was delegated to it
- https://crank.js.org/: generators that yield UI trees
    - components return VDOMs, like react, no imperative mutation

# Conclusion

Representing events as futures instead of using callbacks might be useful, but it's difficult to manage the spawned
tasks. The benefits of awaiting events are clear (easier mutable access to local state), but not sure how often
this will come up in practice.

Instead, events should be callbacks. Focus on a way to avoid cloning and borrowing state across callbacks.

# Callback pain points

1. All state must be wrapped in shared objects / interior mutability, even component local state
2. Capture-by-clone in closures is annoying
3. Leak-by-reference-cycle when capturing strong pointers in closures

For (2): ideally widget refs would be Copy+'static but that's probably impossible.

Alleviating the pain: event handlers should have the form `(&impl EventTarget, impl FnMut(&EventTarget))`.
I.e. the event handler takes a reference to the state (i.e. the receiver).

Callbacks now will not be invoked with local state in scope (i.e. parent state do not appear in the call stack,
so it's not possible to borrow state safely like in previous versions of kyute).

Could it be possible to invoke callbacks with parent state in scope? Not if widgets are `Rc`.

Proposal:

- there's a TLS arena containing elements
- elements have associated IDs, and can be accessed by IDs
- the arena holds the hierarchy (ID -> parent ID) and can be used to enumerate child elements
- **elements don't hold their children**
- however, elements can access their children at any time from the arena

# Avoiding Rc?

Typically, the way to avoid that is to let the container wrap the widget in Rc.
However, this moves the widget into the container, and the caller cannot access it anymore.

Rc isn't absolutely necessary. However, if not using Rc, then we need a way to address specific widgets in the tree.
Weak pointers from Rc are an easy way to do that.

## Raw pointers

Raw pointers could be used if we can guarantee that the widget is still there (and hasn't moved!)
when upgrading the raw pointer to a borrow. This is impossible if the user owns the widget (even with only a read-only
borrow it's possible to use interior mutability to move the referenced widget out of its slot).

So, when referencing a child widget somewhere, it must be owned by the framework. The user should only see a borrow
of the widget. This means that to store it in a struct, lifetime annotations will be required.

Thus, (no Rc + direct pointer to widgets) => lifetime annotations.

## IDs

Need a way to resolve them to references, which is less direct than raw pointers.
It would probably take the form of a generational arena in which all elements are associated to an ID.
No allocation benefit compared to raw pointers since widgets need to be boxed before being put in the arena.
Borrowing from the arena is complicated if there's no borrow of the arena currently accessible.

## Container-owns

The druid approach. Containers directly own their children, sometimes wrapped in additional state (`WidgetPod`).
Widgets have IDs. To send something to a widget by ID, need to traverse the tree and find the matching ID, which is
costly. Druid has a bloom filter to accelerate the traversal, but it's not very convincing.

## Hybrid Rc + index

A lot of UI subtrees are static: their structure never change. Widgets in these static subtrees can be allocated
together and referred to by a fat pointer `(Rc<Subtree>, usize)` (more like an obese pointer, since the subtree pointer
is already fat), where the usize is the static index of the element in the subtree.
Slint does that, I think.

## Alternative: no access to the UI tree once it's built

Model UI as a one-way function that looks at the state and produces a UI tree representation, then diffed against the
retained UI tree. Like xilem, flutter, and the previous kyute incarnation.
Retained UI nodes are never manipulated directly. Instead, the builder function is called again whenever dependent state
is modified.

It's a well-established pattern at this point.

Conclusion:

- if event handlers need direct access to widgets (i.e. call methods on widgets directly), then those need to be `Rc<>`
- otherwise, in a purely reactive approach (where widget trees are rebuilt on state change), widgets are "value types"
  that cannot be referenced.

## Direct reference to parent widgets in event handlers

If parent widgets own their children, it should be OK for an event handler in a child widget to borrow stuff from the
parent, since it's guaranteed to be alive (provided the parent is pinned in memory). We do end up with a
self-referential structure but at least it doesn't need Rc. Borrows that escape the subtree are still impossible
however.

# Conclusions

- (STRUCT-INIT-SYNTAX) is most likely impractical to build a retained widget tree, because retained widgets need to
  track additional (private) state that would show up in the initialization expression. It is more also more suited
  for reactive systems that rebuild subtrees (coarse-grained incrementality) instead of updating the tree (fine-grained
  incrementality).
- (NO-ALLOC) is not a priority since it's unlikely that performance will be a significant issue given the low number of
  widgets on the screen. It could be implemented after as an exercise.
- (DIRECT-WIDGET-REFS) is an implementation detail of reactivity. If we target the "second kind" of reactivity (
  fine-grained updates without diffing) then direct references are necessary.

# Async handlers VS callbacks?

Practical considerations:

- a `Handler<bool>` is 80 bytes
- one `Box<dyn Fn>` is 16 bytes

Is it possible to implement async events via callbacks?
I.e.

```
// Presentation
VBox {  // __1
    Text {  // __2
        value: self.text,
    }
    Button {    // __3
        on_click: self.click_increment,
    }
    Button {    // __4
        on_click: self.click_decrement,
    }
}
```

To get values from the context: `fn get(name: &str) -> &Watch<dyn Any>`

# Minimizing Rcs

`Node` doesn't hold children as `Rc<dyn Element>` anymore (only the pointer to the parent).
Instead, a pointer to an element is represented as a `(Pin<Rc<dyn ElementTree>>, index)`,

```
impl Button {
    async fn await_on_click(&self) {
        let clicked = Cell::new(false);
        // scoped callback that can borrow from the scope
        let _callback = self.on_click.scoped_watch(|| {
            clicked.set(true);
        });
        while !clicked.get() {
            yield_now().await;
        }
        // ISSUE: unsafe since we can mem::forget the _callback
    }
}
```

Things we need from references:

- comparable and sortable
- downgrade to weak
- **create from borrow**: given `&T` where `T: Element`, upgrade to `Rc<T>`
- pinned guarantees

# Issues

## `set_focus` reentrancy and panics

During event dispatch it's possible to call `set_focus` on a widget, which immediately sends another event.
This can lead to panics if the `event` method borrows a RefCell mutably in the function scope, and the `event` handler
is called reentrantly.

Maybe these methods should take a `&mut self` instead? It would mean wrapping all widgets in `RefCell`, and doesn't
actually prevent reentrancy panics.

Q: Are direct references to widgets necessary?
A: Without direct references, no async syntax is possible.
Everything must be done via callbacks, in a reactive fashion, or via intermediate objects watched by GUI widgets
(NOTE: this is most likely the "ViewModels" of the MVVM pattern).
In fact, it could be said that a characteristic difference between reactive and imperative GUI frameworks is the ability
to refer to GUI elements by reference and invoke methods on them.

Q: Is that really a problem? The DOM specifies that "Additional events should be handled in a synchronous manner and may
cause reentrancy into the event model".
In our case, it might panic with interior mutability, but at least the issue will be obvious when it happens.

## Too many layout size types

* FlexSize
* SizeConstraint
* Sizing
* SizeValue
* LengthOrPercentage

# A simpler layout system

Main use cases:

* center one element within another
* being able to size a box to its contents, but with a minimum size constraint
* layout items in a flex container

Unnecessary for now:

* min-content sizing

Something like morphorm would work.

Sizes are specified on individual items, each size has a min/max/preferred constraint.

```
pub enum Size {
    /// Percentage of parent size
    Percentage,
    /// Size to contents
    Auto,
    /// Fixed size in pixels
    Fixed(f64),
    /// Amount of remaining space
    Flex(f64)
}

pub struct SizeConstraints {
    preferred: Size,
    min: Size,
    max: Size,
}
```

Issue: what does a "flexible minimum size" means?
In morphorm: returns default value.

Modification to `measure`: now returns a size **and** a stretch factor?
No => should return a concrete size.

In what measure should the child be responsible for measuring itself?
If it has a fixed size, then it makes sense that it's the child, but with flex factors / percentages, the information
to compute the size is held by the parent.

## Specification

The size of a node is determined by its `measure` method.

For frames, which is a generic container for other elements and a basic visual primitive, the size is determined by
its `width` and `height`, `min_width`, `min_height`, `max_width` and `max_height` properties.
Those values are of the type `Size`, and can represent one of those:

- a fixed size
- a percentage of the parent container size
- a special value that specifies the size of the contents of the frame
- a special value including a flex factor that specifies a proportion of the parent container size once all non-flex
  elements have been laid out

The `measure` method takes as input:

- the available size for layout, in both directions

## Layout: measure model

Currently the measure function returns a size given a proposal. However, this is not suited for things like flex items
which take a proportion of the remaining space.
It is not suited because the flex child can only return one size in `measure`, and cannot specify that it can grow.

Idea: adding flex growth as a return value of `measure`?
A: yes, but not sufficient: the child might have a max-width constraint. The return value of `measure` doesn't specify
that, thus the parent container may end up allocating a size for that child that exceeds its max width constraint.

Idea: `measure` returns a range of possible values: minimum size, maximum size, with a growth factor that specifies
how the flex item should grow in the remaining available space.

Alternatively: call `measure` multiple times with different "available space" proposals to get the min/max/preferred
sizes.
For flex children, the parent container will read the attached property to determine the flex growth factor from
min to max.

Example with text:

* `Text.measure(0., 0.)`: returns the minimum size (width = width of the longest word, height = height of paragraph
  given width)
* `Text.measure(+inf,+inf)`: returns the maximum size (width = width of the longest line)

Example with a frame that has a min-size

* `Frame.measure`

Issue: flex factors, again. It's possible to specify a `Stretch` size on both axes, but it doesn't make sense
on the cross axis (it's ignored), or if the frame isn't in a flexible container at all.

=> `Stretch` should make sense all the time, not only contextually.

The same is true in the layout protocol: `Measurements::flex` is ignored when the parent isn't a flex container.
Plus, it makes the API more difficult: custom container widgets now need to think about how to handle flex widgets.

### Alternative proposal

Don't expose flex in returned measurements. Instead, call measure multiple times to determine the min & max sizes
of the widget. Flex layout becomes:

First, determine whether the child element is flexible. This is done by measuring it with unbounded (+inf) constraints
on the main axis.

For each child,
call measure with unbounded constraints.
If the returned size is infinite, consider it flexible.
Otherwise, measure it immediately

### Alternative (2) proposal

Issue: the container should know which proportion of the available space should be allocated to the child **before**
calling `measure`.

`measure` returns size with constraints applied, _and_ flex factors for both dimensions.
E.g.

- `400x200, (*1,*0)`: 200x200 base size, flexible at 1x in the horizontal direction
    - but the `400` means nothing here if it's flexible!
    - then `*1 x 200`
        - this doesn't take into account any minimum/maximum constraint on the size of the element

Measure cannot return a range of sizes. If it could, returned measures should be able to fully model the sizing behavior
of the child item, and the available space passed to `measure` would be useless.

Idea: maybe, alongside the measure, the widget could return a "priority" value that tells the container how to resolve
conflicts with other widgets if it leads to overflow.

# Hot reload

Proc-macro that watches the original source files for changes.
The proc-macro produces the AST of the UI tree, which is then interpreted to:

- create the UI hierarchy
- set the value of properties

The proc-macro also generates code to bind widgets to local variables.

# Style overrides that depend on the element state

Option A: they aren't necessary, instead set-up event handlers that change the style when the element state changes.

- Less complexity in elements
- Less concepts
- More code necessary when using elements
- Harder to specify declaratively
    - Need a scripting language in expressions and reactive dependencies

Option B: pseudo-classes like CSS

- More complexity in elements
- More concepts
- Can be specified declaratively

# Template language

A template language would be one possible solution to two problems:

1. hot-reload
2. the rust syntax/API to build UI trees being very imperative and not super readable (like Qt widgets, basically)

Hot-reload being the most important.

Problem #2 is because we're not adopting a "reactive" approach to UI: instead of generating new UI trees on model
change, we patch elements in the tree instead. This means that we directly reference items in the trees.

Support simple reactive expressions. Necessary for stuff like changing styles on hover, etc. Support **hot-reload** as
well.

How to?

Option A: custom expression language + interpreter

- dangers: scope creep, documentation, maintenance of a programming language, no autocompletion

Option B: rust syntax in a proc-macro, but compiled to WASM

- It's basically a macro that generates rust code, and compiles it to WASM on the fly
- Still no autocompletion (because code will be inside a macro)

Option C: javascript runtime

# Reactive stuff?

```

let increment = ...;
let decrement = ...;

emit(Frame {
    Button {
        on_click: increment,
        ..
    },
    Text {
        text: text!("{counter}")
    }
    ..
});

increment.await;

```

Issue: elements like `Text` or `TextEdit` cannot be bound to reactive variables.

There must be a layer on top that takes care of registering the callbacks on the model to update the state. But
this should not be in the implementation of the widget.
Something that turns a declarative description like `Text(text: value)`
into imperative code:

```
let __elem = Text::new();
__elem.set_text(value.get());
value.on_change(__elem, |e, value| e.set_text(value));
```

This can be done with macros, or via a declarative layer on top of the element tree.
For instance, UI functions could return a view tree, that is then "applied" on top of a corresponding retained
element tree, recursively. Hot reload would be complicated.
This is very much like xilem.

# Container-owns tree

Issues:

- (STRUCTURE) flat VS container owns VS component owns
- (DISPATCH) dispatching closures to children
- (IDS) identifying nodes
- (DIRTY) dirty flag propagation: when a widget needs to relayout/repaint, propagate this information to the parent
- (NAV) navigate to the previous/next element in the tree

Structure:

- flat: elements are stored in a flat generational table; elements can be referred by indexing in the table
- ~~container-owns~~: no table, containers own their child elements; elements are referred by ID, but containers must
  implement methods to find child IDs
    - containers have mutable access to their child elements
        - not sure if that's interesting in practice
    - implementors must worry about finding child IDs => more code for the authors, avoid
- component owns: within a component, elements are stored as fields in a struct, regardless of their parenting
  relationships
    - component has mutable access to all elements, even those several layers deep in the hierarchy
    - hierarchy is encoded separately
    - components themselves are stored in a table
    - no allocation needed for static nodes

For (DIRTY):

- flat: can't access parent directly, use a separate context
- container-owns: same, use a context
- component-owns: same

For (IDS): use a randomly generated ID, or just use a slotmap generational ID.

Decision: switch to a flat map. Xilem/masonry does the same. "component-owns" approach isn't worth the added complexity
(even if it is mostly hidden from the user).
Widget manipulation methods should take a special receiver with traversal context, like masonry.
However, unlike masonry, containers don't hold the list of their children.

# Applications with complex GUIs

- Autodesk Maya: uses Qt
- Ableton: custom, closed source, not extensible
- Affinity Designer: possibly custom C# or C++, closed source
- Sublime Text: possibly custom, rendering with skia, closed source

# Practical GUI visual design

Ideally, would use something like illustrator instead of doing stuff in code. However, very difficult
to do responsive stuff in software like that.

Alternative: a mini drawing language that can be hot-reloaded.
Contains drawing instructions.

Q: how to pass information from Rust to the drawing instructions?
Q: how to embed rust code in it?
Q: interactivity?
Q: how to embed other components?

```

Input:
- drawing region (rectangle)
- 

Variables:
- center

Declarations:
- transform blocks
- clip regions


{
    #rect(bounds) {
        #fill(FILL_COLOR)
        #stroke(1px, inner, STROKE_COLOR)
        #box_shadow()
    }
    
    
    line(from: centerLeft, to: centerRight, 2px, center, STROKE_COLOR)
    
    #line(from: topLeft, to: bottomRight, 1px, center, STROKE_COLOR)
    #line(from: bottomLeft, to: topRight, 1px, center, STROKE_COLOR)
    
    #if(hovered) {
        #rect(bounds) { #fill(FILL_COLOR_HOVERED) }
    }
    
    rect(bounds) + Fill { color: FILL_COLOR } + Stroke { } 
        .fill(FILL_COLOR)
        .stroke(1

}


{

    rect(id=RECT, align=top_left, anchor=top_left, bounds, fill=FILL_COLOR);
    
    border(1, inside, rgb(255, 0, 255));
    
    group(opacity=0.5) {
        
    }
}


```

Drawing a button:

- centering text inside a region
- drawing a border inside a shape
- drawing a box shadow inside a shape
- conditionals
- gradients

```rust

fn draw(cx: &mut DrawCtx) {
    use kyute::drawing::prelude::*; // Inside, Center, Outside, rgb

    // set rounding around edges of the current shape
    cx.border_radius(8);

    // draw a 1px border inside current shape 
    cx.border(1, Inside, rgb(255, 0, 255));

    // fill the current shape with a pattern, in this case a solid color
    let hovered = cx.hovered();
    cx.fill(if hovered { rgb(...) } else { rgb(...) });

    // drop shadow

    // sets the baseline of the containing block.
    cx.baseline(80);

    // centered label
    // Specify alignment within the containing box: 
    // horizontally centered, baseline positioned 80px below top
    cx.label(TextCentered, Baseline, text!["OK"]);  // TextItem

    // equivalent:
    let label = cx.make_label(text!["OK"]);
    let pos = cx.centered(label.cap_height_bbox());
    cx.draw_label(pos, label);

    // gradients:
    let g = gradient(cx.h_midline(), [rgb(...), rgb(...)]);

    for i in 0..1 {
        cx.draw(vg! {
            rect(align=top_left, 
                 anchor=top_left,
                 radius=3, 
                 baseline=10, 
                 fill={g},
                 border=(1, inside, rgb(...)),
            )
            [
                text(align=baseline, anchor=baseline, ["OK"])
            ]
            
            grid(
                rows=[1,1fr,1],
                columns=[1,1fr,1],
            )
            area(1,1) {
            
            }
            
        });
    }


    // Expands to:
    // 
    [Item::Rect {
        align: Anchor::TopLeft,
        anchor: Anchor::TopLeft,
        radius: 3,
        baseline: 10,
        fill: &g,
        border: (1, Border::Inside,),
        content: &[
            Item::Text {
                align: Anchor::Baseline,
                text: text!["OK"],
                ..Default::default()
            }
        ],
        ..Default::default()
    },
        Item::Grid {
            rows: ...,
            columns: ...,
            content: &[
                GridArea {
                    row_span: ...,
                    col_span: ...,
                    content: &[],
                }
            ],
        }
    ]

    // Issues: this borrows stuff from the surrounding scope, it must be used immediately, 
    // and cannot be stored in a variable unless serialized


    // Expands to a serialized drawing, or at least a block of code that draws the thing
}

```

Considerations:

- making a procedural drawing DSL is far too complicated and requires too much maintenance
- an attainable goal would be to make a macro for more concisely specifying shapes and fills and groups, like SVG
    - basically, a macro to produce an inline representation of SVG, with parameters interpolated from the surrounding
      context


# Next steps
- Context menus have to close when they lose focus
- Better API (wrapper) for monitor size
- Submenus
- Better event handling: right now it's only convenient to listen to events from widgets _and_ modify the same widget (the `on` method).
  - There should be a way to watch events from any target (OK), and to add callbacks that take a mutable reference to the target.
    - This is only convenient with `ElementBuilder::on()`

# Event emitter unification

- Have `Window` be an `EventEmitter`
- Figure out how we avoid (or not) borrowing errors with `ElementRc`
  - when do we need to use `run_later`?
  - what we must guarantee is to correctly propagate flags up the tree when invoking a method on an element
    - but recursively calling `ElementRc::invoke` on a child element won't work, because the parent element is borrowed 
    - Solution: disallow direct calls to `invoke` on child elements, 
      - all calls to invoke must be in a `run_later` closure to ensure no outstanding borrows
      - but: how to enforce that? invoke must be made private, and there needs to be specializations of `run_later` 
        for `ElementRc`
        - too complicated
  - allow direct calls to child elements, using borrow_mut
    - it's the responsibility of the caller to avoid reentrancy
    - usually won't be a problem when responding to events
  - the `ElemBox::callback` pattern may be generalizable to other `Rc<RefCell>`-like objects

- `ElemBox::callback` not super intuitive, doesn't work with callbacks that get multiple parameters
  - also it doesn't work with closures that take a reference because it's impossible to make it generic (the type parameter would have to be a HKT)
  - maybe a macro instead?

# Getting rid of parent pointers?

May be possible. But there needs to be an alternate way of building an explicit hierarchy, because it's necessary 
to propagate dirty flags up the tree.
Alternative: IDs

- each element has a unique ID (generational index)
- when an element is added to a parent, update the global parent->child relationship table
    - when an element is dropped, remove the key from the table

Instead of allocating with Rc, allocate in generational table (boxed).
ElementBuilder is an owned pointer to the element (like box). 

On a side table, store child->parent relationships.
On a side table, store dirty flags.

`ElementRc` becomes a single pointer to the element table `ElementRef<T>(usize)`, and it signifies ownership.
Thus it is not copyable. However, to access an element you need an exclusive reference to the table, with a separate `&mut ElemBox`.

`WeakElement` is a generational index. It can be upgraded to a `&mut ElemBox`, but this locks the element table for writing.

`&mut Node<Self>` is a mutable reference to an entry in the element table (i.e. a mutable reference to an element).
It can be used to access other elements in the tree, given a `&mut ElementRef`.

```

    // `Aliasable` needed because `Element` methods get `&mut ElemBox`, because elements get exclusive
    // access to the element, but we **don't** want to give exclusive access to `ElementCtx`.
    //
    // This leaves us with a few options:
    // - store ElementCtx outside the ElemBox, and remove ElemBox
    //      - this is not great because now methods have two parameters (`&mut Self, &ElementCtx`)
    // - retrieve the ElementCtx from a weak self-reference inside the element
    //      - not super ergonomic, not efficient because we need to upgrade the weak reference
    // - store ElementCtx inside the ElemBox, but make it `UnsafePinned`
    //      - we pass only one pointer (`&mut ElemBox`)
    // - pass `&mut ElemBox` but store the ElementCtx just before in memory
    //      - uncheckable by miri, needs exposed provenance
    // - wrap ElementCtx in Rc, parent pointers point to this Rc
    //      - needs a separate allocation, and another indirection to access ctx + extra rc clones
    //      - parent pointers point to the ElementCtx
    //      - blame the rust memory semantics for this crap
    // - Store a raw pointer to the ElementCtx in the ElemBox
    //      - the raw pointer points to the ElementCtx behind the ElemBox
    //      - the raw pointer has shared read permissions on the ElementCtx
    //      - redundant data, necessary to appease miri
    //      - self-referential, so will probably need pinning
    //      - => **can't work**, pinning means we can't have naked `&mut ElemBox`es (only `Pin<&mut ElemBox>`)
    //           and the ergonomics of that are completely unacceptable
    //      - without pinning, it's possible to make things unsound by swapping `ElemBoxes` as they
    //          are built inside ElementBuilders (since ElementBuilder derefs-mut to ElemBox)
    // - Store a raw pointer to the ElementCtx in the ElemBox, parent pointers point to this ctx
    //      - its lifetime is tied to the ElemBox
    //      - child elements point to this ElementCtx, but must be careful to also hold a weak ref to the parent
    //      - the ElementCtx can't be stored alongside ElemBox because it could be moved
    //      - the ElemBox "owns" the ElementCtx but doesn't have exclusive access to it
    //      - still doesn't work: possible to swap ElemBoxes when a parent link has been established
    // => Rule: parent links **cannot** point to something inside an elembox, be it either a field or a raw pointer owned by the elembox
    // - Store the ElementCtx in a UnsafeAliased cell, get a pointer to it **without** constructing a mut reference


    /// **NOTE**: access to this field may be aliased, even when accessed through an exclusive reference
    /// to the `ElemBox`. The reason for that is a result of several considerations:
    ///
    /// 1. We want `Element` methods to receive both the element and its context.
    /// 2. `Element` methods should receive an exclusive reference to the element (`&mut self`).
    /// 3. `Element` methods have only one parameter for the element and the context (`&mut ElemBox<Self>` which bundles both the element and its context).
    ///    This is because we want those methods to also work with the element as it is being built (via `ElementBuilder`) and it's not ergonomic to have to
    ///    construct a dummy context at this point. Since `ElementBuilder` deref-muts to `ElemBox` this is no problem.
    /// 4. Element contexts must be shared because it's possible for a child element to call `mark_needs_paint`
    ///    (which accesses the contexts of parent elements) while a parent element is exclusively borrowed.
    ///
    /// So we have this situation where:
    /// - the element and context are stored together
    /// - methods take an exclusive reference to the element
    /// - methods should only take one parameter for both the element and the context, thus `&mut Something` where `Something` bundles both the element and the context
    /// 
    /// Note: if (3.) wasn't a requirement, we could store the context outside the `ElemBox`
    /// and pass it as a separate parameter to `Element` methods. This would make those methods
    /// incompatible with `ElementBuilder`, but that might not be too important.
    /// 
    /// Another option is to store the context inside the Element itself, but it would still need to be aliased.
```

# TODO
- figure out layout rounding policies.
  - it's already a problem for menus: MenuBase::measure returns a fractional size which is used as a window size, and
    rounded to the nearest integer. This can lead to a window that is too small or too large.
    - Q: should MenuBase::measure return a rounded size?
    - Q: should the parent container round the size of its children?
    - Q: should neither of them round, and let Window should round the size of the window?
  - Q: in what units should windows be sized? logical pixels or physical pixels?
    - A: it should be logical, like everything else.
 - A: no rounding of layouts, but paint methods may round to pixels if they want
   - also, paint methods should receive bounds in window coordinates, not coordinates relative to the parent
   - this way paint methods can round to device pixels regardless of the current transformation on the canvas, 
     because there is *no* transformation on the canvas
   - remove arbitrary affine transformations on widgets
     - scaling, rotations are largely untested so far
     - unlikely to be of any use in the future
       - arbitrary transforms will be used in zoom&pan views, but they are separate from the widget hierarchy


# Q: where/how to keep references to compositor layers

Use case: a widget that provides its own compositor layer. The layer must be inserted in the tree, which means
that the widget must retrieve a reference to the parent layer in the UI tree. Ideally the layer should be inserted 
when the widget is parented to a window (mounted). 
Thus, the widget must have access to some data belonging to the parent.

Options:
- (A) store `Layer` in `ElementCtx`
  - unergonomic: the widget doesn't directly own the layer, `ElementCtx` must be passed by mutable reference (tricky)
  - unergonomic: it must be stored as `Option<Layer>`, which adds syntactical overhead on every access (`.as_ref().unwrap()` everywhere...)
- (B) store `Layer` in the Element itself, and pass down a reference to children via `TreeCtx`
  - impossible, because we need to be able to build a `TreeCtx` "from the middle" of the tree, we don't always build the `TreeCtx` chain from the root
  - unergonomic: the widget with a custom layer must override all methods with `TreeCtx` to pass the layer down
- (C) make `Layer` a clonable refcounted type, store `Option<Layer>` in `ElementCtx`, widgets can hold their own reference.
  - sanest option; most backend frameworks (DirectComposition, AppKit) implement layers (and many other things) as refcounted objects

What would other languages do?
- in C#, `Layer`s would have reference semantics, and the layer would be stored in a nullable field in the base class
- in Swift/ObjC, same thing (see `NSView::layer`)
  - this would require `Layer` to be a refcounted reference type and be clonable

Conclusion: not worth the hassle to have unique ownership semantics to `Layer`. 
Go with option (C) and don't think about it anymore. 


# TODO
- Fix issue with layout in `Text` with flex containers when `measure` doesn't return the same size?
  - The issue: 
    - Measurement is performed first to determine the size of the window
       - During the measure phase, the text returns a width of 650.4 under unspecified constraints (infinite width available)
       - The window is then created with a width of *650* because of *truncation somewhere* (**issue 1**)
    - During the layout phase, the text is *measured again* by the flex layout container, this time with 
      a width constraint of 650. This causes the text layout to split the text into two lines, and the height measurement
      is now bigger than the initial measurement before creating the window 
    - Because of the taller text, the contents now overflow the window.
    - During painting, we first clear the background of the window. This implicitly allocates a DrawSurface with the size of the window.
    - When we paint an overflowing element, the DrawSurface is not big enough to display the text, 
      which causes a new DrawSurface (and a new compositor layer) to be allocated with the size of the text (**issue 2**).
      - Currently, compositor layers are opaque, so all the content painted before the overflowing element is lost (**issue 3**).
  - Solutions:
    - Issue 1: when sizing a window to contents, round the size to the next larger integer value.
       - or, alternatively, automatically round all sizes returned by `measure` to the smallest enclosing pixel-aligned rectangle
         - no
    - (OK) Issue 2: we shouldn't attempt to create a compositor layer that exceeds the bounds of the parent window. 
               This should be enforced by `CompositionBuilder`
    - Issue 3: compositor layers should have alpha
       - TODO: this has implications on input latency 

- Need to be able to inspect the layout tree. 
   - Either dump to terminal or view it in a separate window.

- Fix initial position of modal windows 
  - They flash in the top-left corner of the screen before being moved to the correct position.
  - Not sure if this is a problem with winit or with us

- Separate type for UI tree roots
- Module reorg
  - element.rs, node.rs, focus.rs into a common intermediate module
- Remove recursive hit_test method