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
stuff like referencing nodes by ID or even ID paths (lenses), with additional indirections,
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
