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

## Alternative design

- With arbitrary_self_types, methods on `T` get `self: &Element<Self>`, i.e. it's inside-out (the Element owns the
  derived
  type).
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