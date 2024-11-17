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

## Direct references to elements in the UI tree

Previously elements were directly owned by their parents. It was impossible to keep a borrow to an element for long
(the lifetimes would be intractable), so elements had a unique ID, and could be reached by ID paths from the root to the
element.

This ended up complicating the implementation of the widget trait (widgets needed to "collaborate" to deliver events to
the proper child widget), so don't do that again. Instead, it should be possible to directly refer to a widget by a
reference, most likely `Rc/Weak` pointers.

## Context-less widget methods.

Methods of widgets (for instance: `TextInput::move_cursor`, `Checkbox::is_checked`, `Button::set_focus`) shouldn't need
a `Ctx` parameter to work correctly. We should be able to call those methods directly, in any context, given
a reference to the widget (e.g. `text_input.move_cursor_to_next_word()` instead of
`text_input.move_cursor_to_next_word(ctx)`).

Rationale: in a previous design, callback-taking methods needed a `Ctx` instance. This is pure syntactical noise for
the user of the library. We should strive to eliminate any "accidental concepts" that don't bring any control to the
user of the library.

A practical consequence of that is that it should be possible to obtain a shared reference to a widget from `&self`.
(So that it's possible to use methods that store strong references to widgets).

## No complicated wrapper types and non-standard receiver types.

Avoid methods with signatures like `fn method(self: &Rc<Self>)`. This is surprising, unergonomic. Prefer storing
a cyclic weak pointer inside the object so that it can be upgraded to Rc when needed.

## Minimize the need for `Rc` wrapping in user code

If possible, avoid the need for wrapping in Rc for custom widgets: it should be `Widget::new() -> Widget`, not
`Widget::new() -> Rc<Widget>`. If it's necessary, wrap with `Rc` in `add_child`.
Why? Minimize noise when building deep UI trees.

## Hot-reloading

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

## No "virtual DOM"

Avoid virtual DOM approaches that require reconciliation. The retained tree should be directly, and efficiently mutable.

## Async event handling

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

# Separating behavior from presentation:

Presentation:

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
