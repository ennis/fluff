//! Rebuilds a subtree of elements when a model dependency is changed.

use kurbo::{Point, Size};
use crate::{Element, PaintCtx};
use crate::element::{ElementAny, ElementBuilder, Container, ElementCtx, ElementCtxAny, HitTestCtx, IntoElementAny, WeakElementAny, WeakElement, ElementRc};
use crate::layout::{LayoutInput, LayoutOutput};
use crate::model::{with_tracking_scope, ModelChanged};
// Issue: the builder is in the element tree but it serves no purpose
// in the tree (it doesn't participate in layout, doesn't draw anything, doesn't handle events).
// It would be better if this wasn't an element but a separate thing that can be used to build elements.

// Challenge: the callback to rebuild the element would have to modify the parent widget.

// Conditional builders don't even produce an element every time.

// Idea:
// - the initial ElementAny is built
//
// Containers don't take a IntoElementAny, but rather some kind of builder that inserts/removes elements.
// Issue: it needs to remember at which place the elements should be inserted. Which is complicated
// because there might be elements added before or after.

/*pub struct Builder<F> {
    ctx: ElementCtx<Self>,
    content: ElementAny,
    builder: F,
}

impl<F, E> Element for Builder<F>
where
    F: FnMut() -> E,
    E: IntoElementAny,
{
    fn ctx(&self) -> &ElementCtxAny {
        &self.ctx
    }

    fn ctx_mut(&mut self) -> &mut ElementCtxAny {
        &mut self.ctx
    }

    fn measure(&mut self, layout_input: &LayoutInput) -> Size {
        self.content.measure(layout_input)
    }

    fn layout(&mut self, size: Size) -> LayoutOutput {}

    fn hit_test(&self, ctx: &mut HitTestCtx, point: Point) -> bool {
        todo!()
    }

    fn paint(&mut self, ctx: &mut PaintCtx) {
        todo!()
    }
}*/


pub trait ElementSequence<Item> {
    fn items(&self) -> Vec<Item>;

    fn collect_items(&mut self, out: &mut Vec<Item>) {
        out.extend(self.items());
    }
}

pub trait ElementBuilderSequence<Item> {
    type Elements: ElementSequence<Item>;

    fn into_elements<C>(self, parent_container: WeakElement<C>) -> Self::Elements
    where
        C: Container<Item, Elements=Self::Elements>;
}


impl<Item> ElementSequence<Item> for () {
    fn items(&self) -> Vec<Item> {
        vec![]
    }
}

impl<Item> ElementSequence<Item> for Vec<Item>
where
    Item: Clone,
{
    fn items(&self) -> Vec<Item> {
        self.clone()
    }
}

impl<Item> ElementBuilderSequence<Item> for () {
    type Elements = ();
    fn into_elements<C>(self, _container: WeakElement<C>) -> Self::Elements {
        ()
    }
}

impl<T> ElementSequence<ElementAny> for ElementRc<T>
{
    fn items(&self) -> Vec<ElementAny> {
        vec![self.clone()]
    }
}

impl<T: Element> ElementBuilderSequence<T> for ElementBuilder<T>
{
    type Elements = ElementAny;
    fn into_elements<C>(self, container: WeakElement<C>) -> Self::Elements
    where
        C: Container<ElementAny, Elements=Self::Elements>,
    {
        self.into_element(container, 0)
    }
}

macro_rules! impl_element_sequence_tuple {
    (
        $(
            $name:ident
        ),*
    ) => {
        #[allow(non_snake_case)]
        impl<$($name,)* Item> ElementSequence<Item> for ($($name),*)
        where
            $($name: ElementSequence<Item>),*
        {
            fn items(&self) -> Vec<Item> {
                let ($($name),*) = self;
                let mut items = vec![];
                $(
                    items.extend($name.items());
                )*
                items
            }
        }

        #[allow(non_snake_case)]
        impl<$($name,)* Item> ElementBuilderSequence<Item> for ($($name),*)
        where
            $($name: ElementBuilderSequence<Item>),*
        {
            type Elements = ($($name::Elements),*);

            fn into_elements<Ct>(self, parent: WeakElement<Ct>) -> Self::Elements
            where
                Ct: Container<Elements=Self::Elements>,
            {
                let ($($name),*) = self;
                ($($name.into_elements(parent.clone())),*)
            }
        }
    };
}

impl_element_sequence_tuple!(A, B);
impl_element_sequence_tuple!(A, B, C);
impl_element_sequence_tuple!(A, B, C, D);
impl_element_sequence_tuple!(A, B, C, D, E);
impl_element_sequence_tuple!(A, B, C, D, E, F);
impl_element_sequence_tuple!(A, B, C, D, E, F, G);
impl_element_sequence_tuple!(A, B, C, D, E, F, G, H);


impl<F, Seq, Item> ElementBuilderSequence<Item> for F
where
    F: FnMut() -> Seq,
    Seq: ElementBuilderSequence<Item>,
{
    type Elements = Seq::Elements;

    fn into_elements<C>(mut self, container: WeakElement<C>)
    where
        C: Container<Elements=Self::Elements>,
    {
        let (r, tracking_scope) = with_tracking_scope(|| self());
        {
            let container = container.clone();
            tracking_scope.watch_once::<ModelChanged, _>(move |source, event| {
                if let Some(c) = container.upgrade() {
                    c.invoke(move |c| {
                        *c.elements() = self.into_elements(container);
                        c.ctx_mut().mark_needs_layout();
                    });
                }
                false
            });
        }
        r.into_elements(container)
    }
}