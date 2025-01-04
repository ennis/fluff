use slotmap::{new_key_type, SlotMap};
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};

new_key_type! {
    pub struct NodeKey;
}

struct NodeHeader<N> {
    this_key: NodeKey,
    tree: *mut NodeMap<N>,
    parent: *mut Node<N>,
    // Could be a linked list instead
    children: Vec<*mut Node<N>>,
    index_in_parent: usize,
}

struct Node<N> {
    h: NodeHeader<N>,
    inner: N,
}

impl<N> Node<N> {
    fn alloc(tree: *mut NodeMap<N>, key: NodeKey, inner: N) -> *mut Self {
        Box::into_raw(Box::new(Node {
            h: NodeHeader {
                tree,
                this_key: key,
                parent: std::ptr::null_mut(),
                children: Vec::new(),
                index_in_parent: 0,
            },
            inner,
        }))
    }
}

type NodeMap<N> = SlotMap<NodeKey, *mut Node<N>>;

pub struct Tree<N> {
    // We don't use Box here because pointers to the map in `Node`s would be invalidated
    // when moving the box (see https://doc.rust-lang.org/std/boxed/index.html#considerations-for-unsafe-code
    // and https://github.com/rust-lang/unsafe-code-guidelines/issues/326).
    // Instead we allocate the map with Box::new and immediately turn it into a raw pointer.
    // We turn it back into a Box when dropping the tree.
    nodes: *mut NodeMap<N>,
}

impl<N> Drop for Tree<N> {
    fn drop(&mut self) {
        unsafe {
            let mut nodes = Box::from_raw(self.nodes);
            nodes.retain(|_, node| {
                let _ = Box::from_raw(*node);
                false
            });
        }
    }
}

unsafe fn detach_node<N>(node: &mut Node<N>) {
    unsafe {
        if let Some(parent) = node.h.parent.as_mut() {
            parent.h.children.remove(node.h.index_in_parent);
            for i in node.h.index_in_parent..parent.h.children.len() {
                let child = &mut *parent.h.children[i];
                child.h.index_in_parent -= 1;
            }
            node.h.parent = std::ptr::null_mut();
        }
    }
}

unsafe fn remove_node<N>(nodes: &mut NodeMap<N>, node: NodeKey) -> Option<N> {
    fn collect_nodes<N>(root: &Node<N>, collected: &mut Vec<NodeKey>) {
        for &child in root.h.children.iter() {
            let child = unsafe { &*child };
            collected.push(child.h.this_key);
            collect_nodes(child, collected);
        }
    }

    unsafe {
        // remove descendants
        let mut to_remove = Vec::new();
        //
        collect_nodes(&**nodes.get(node)?, &mut to_remove);
        for r in to_remove {
            let _ = Box::from_raw(nodes.remove(r)?);
        }

        // remove the node itself
        let mut node = Box::from_raw(nodes.remove(node)?);
        // remove the node from its parent
        detach_node(&mut node);
        Some(node.inner)
    }
}


impl<N> Tree<N> {
    pub fn new() -> Self {
        Tree {
            nodes: Box::into_raw(Box::new(SlotMap::with_key())),
        }
    }

    /// Returns an iterator over the orphans (nodes without parents) of this tree.
    pub fn orphans(&self) -> impl Iterator<Item=NodeRef<N>> + '_ {
        self.iter().filter(|node| !node.has_parent())
    }

    /// Iterates over all nodes in this tree, in no particular order.
    pub fn iter(&self) -> impl Iterator<Item=NodeRef<N>> + '_ {
        unsafe { (*self.nodes).values().map(|node| NodeRef(&**node)) }
    }

    /// Inserts a new root element.
    pub fn create(&mut self, node: N) -> NodeRefMut<N> {
        // SAFETY: we have exclusive access via
        let nodes = unsafe { &mut *self.nodes };
        let key = nodes.insert_with_key(|key| Node::alloc(self.nodes, key, node));
        // SAFETY: no other mutable references to the node exist at this point
        NodeRefMut(*nodes.get_mut(key).unwrap(), PhantomData)
    }

    /// Removes an element and its descendants.
    pub fn remove(&mut self, node: NodeKey) -> Option<N> {
        unsafe {
            remove_node(&mut *self.nodes, node)
        }
    }

    /// Returns a mutable reference to a node.
    pub fn get_mut(&mut self, to: NodeKey) -> Option<NodeRefMut<N>> {
        unsafe {
            Some(NodeRefMut(*(*self.nodes).get_mut(to)?, PhantomData))
        }
    }
}

/// An exclusive reference to a node that can be used to traverse the tree upwards (from parent to children) or downwards (from child to parent).
#[repr(transparent)]
pub struct NodeRef<'a, N>(&'a Node<N>);

impl<N> Clone for NodeRef<'_, N> {
    fn clone(&self) -> Self {
        NodeRef(self.0)
    }
}

impl<N> Copy for NodeRef<'_, N> {}

impl<N> Deref for NodeRef<'_, N> {
    type Target = N;

    fn deref(&self) -> &Self::Target {
        &self.0.inner
    }
}

impl<'a, N> NodeRef<'a, N> {
    /// Moves this reference to the parent node.
    pub fn parent(self) -> Option<NodeRef<'a, N>> {
        unsafe { Some(NodeRef(self.0.h.parent.as_ref()?)) }
    }

    /// Returns whether the node has a parent.
    pub fn has_parent(&self) -> bool {
        !self.0.h.parent.is_null()
    }

    /// Returns the number of child nodes.
    pub fn child_count(&self) -> usize {
        self.0.h.children.len()
    }

    /// Returns the key of this node.
    pub fn key(&self) -> NodeKey {
        self.0.h.this_key
    }

    /// Moves this reference to the first child.
    pub fn first_child(self) -> Option<NodeRef<'a, N>> {
        // SAFETY: the first child is guaranteed to be valid as long as the parent node is alive.
        Some(NodeRef(unsafe { &**self.0.h.children.first()? }))
    }

    /// Moves this reference to the next sibling node.
    pub fn next_sibling(self) -> Option<NodeRef<'a, N>> {
        // SAFETY: parent is necessarily different from `self.0`
        unsafe {
            let next = self.0.h.index_in_parent + 1;
            Some(NodeRef(&**self.0.h.parent.as_ref()?.h.children.get(next)?))
        }
    }

    /// Returns an iterator over the children of this node.
    pub fn children(&self) -> impl Iterator<Item=&'a N> + '_ {
        // SAFETY: the children are guaranteed to be valid as long as the parent node is alive.
        self.0.h.children.iter().map(|&child| unsafe { &(*child).inner })
    }

    /// Moves this reference to the specified node.
    pub fn move_to(self, key: NodeKey) -> Option<NodeRef<'a, N>> {
        // SAFETY: the node is guaranteed to be valid as long as the tree is alive.
        unsafe {
            let nodes = &*self.0.h.tree;
            Some(NodeRef(&**nodes.get(key)?))
        }
    }

    /// Returns an iterator over the parents of this node.
    pub fn ancestors(&self) -> impl Iterator<Item=&'a N> + '_ {
        unsafe {
            let mut current = (*self.0).h.parent;
            std::iter::from_fn(move || {
                if current.is_null() {
                    None
                } else {
                    // SAFETY: the parent is guaranteed to be valid as long as the child node is alive.
                    let parent = &*current;
                    current = parent.h.parent;
                    Some(&parent.inner)
                }
            })
        }
    }
}


/// An exclusive reference to the children of a node.
pub struct NodeChildrenRefMut<'a, N>(*mut Node<N>, PhantomData<&'a mut N>);

// Issue: dereferencing the `*mut Node<N>` will invalidate pointers to `Node::inner`

impl<'a, N> NodeChildrenRefMut<'a, N> {
    /// Creates and inserts a new child node under this node.
    pub fn push(&mut self, node: N) -> (&mut N, NodeChildrenRefMut<'a, N>) {
        unsafe {
            //
            let h = &mut (*self.0).h;
            // SAFETY: we can access and modify the tree as long as we don't invalidate pointers
            // to existing nodes. Since we're creating a new node this won't happen.
            let key = (*h.tree).insert_with_key(|key| Node::alloc(h.tree, key, node));
            let ptr = (*h.tree)[key];
            (*ptr).h.parent = self.0;
            (*ptr).h.index_in_parent = h.children.len();
            h.children.push(ptr);
            (&mut (*ptr).inner, NodeChildrenRefMut(ptr, PhantomData))
        }
    }

    /// Removes the specified child node.
    pub fn remove(&mut self, key: NodeKey) -> Option<N> {
        unsafe {
            let h = &mut (*self.0).h;
            assert_ne!(h.this_key, key, "A node cannot remove itself");
            let child_parent = (*(*h.tree)[key]).h.parent;
            assert_eq!(child_parent, self.0, "The specified node is not a child of this node");
            remove_node(&mut *h.tree, key)
        }
    }

    pub fn key(&self) -> NodeKey {
        unsafe { (*self.0).h.this_key }
    }
}

/// An exclusive reference to a node that can be used to traverse the tree upwards (from parent to children) or downwards (from child to parent).
// NOTE: unfortunately we can't use `&'a mut Node<N>` because miri doesn't like it.
#[repr(transparent)]
pub struct NodeRefMut<'a, N>(*mut Node<N>, PhantomData<&'a mut N>);

impl<N> Deref for NodeRefMut<'_, N> {
    type Target = N;

    fn deref(&self) -> &Self::Target {
        unsafe { &(*self.0).inner }
    }
}

impl<N> DerefMut for NodeRefMut<'_, N> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut (*self.0).inner }
    }
}

impl<'a, N> NodeRefMut<'a, N> {
    /// Moves this reference to the parent node.
    pub fn parent(self) -> Option<NodeRefMut<'a, N>> {
        unsafe { Some(NodeRefMut((*self.0).h.parent.as_mut()?, PhantomData)) }
    }

    pub fn reborrow(&mut self) -> NodeRefMut<N> {
        NodeRefMut(self.0, PhantomData)
    }

    /// Returns the number of child nodes.
    pub fn child_count(&self) -> usize {
        unsafe { (*self.0).h.children.len() }
    }

    /// Returns the key of this node.
    pub fn key(&self) -> NodeKey {
        unsafe { (*self.0).h.this_key }
    }

    /// Detaches this node from its parent.
    pub fn detach(&mut self) {
        unsafe {
            detach_node(&mut *self.0);
        }
    }

    /// Splits this reference into a reference to this element and a mutable reference to its children.
    pub fn split_children(self) -> (&'a mut N, NodeChildrenRefMut<'a, N>) {
        let n = unsafe { &mut *self.0 };
        let inner = &mut n.inner;
        let children = NodeChildrenRefMut(self.0, PhantomData);
        (inner, children)
    }

    /// Inserts this node as a child of another node.
    pub fn insert_as_child(mut self, of: NodeKey) -> Self {
        self.detach();
        unsafe {
            // SAFETY: there is no other mutable reference to the tree (this `NodeRefMut` borrows
            // the tree mutably).
            let this = &mut *self.0;
            let tree = &mut *this.h.tree;
            assert_ne!(this.h.this_key, of, "A node cannot be its own parent");
            // SAFETY: the parent node is guaranteed to be different from `self.0`
            let parent = *tree.get_mut(of).expect("Parent node not found");
            this.h.parent = parent;
            this.h.index_in_parent = (*parent).h.children.len();
            (*parent).h.children.push(self.0);
            self
        }
    }

    /// Creates and inserts a new child node under this node.
    pub fn insert_child(&mut self, node: N) -> NodeRefMut<N> {
        let (_, t) = self.reborrow().split_children().1.push(node);
        NodeRefMut(t.0, PhantomData)
    }

    /// Removes the specified child node.
    pub fn remove_child(&mut self, key: NodeKey) -> Option<N> {
        self.reborrow().split_children().1.remove(key)
    }

    /// Moves this reference to the next sibling node.
    pub fn next_sibling(self) -> Option<NodeRefMut<'a, N>> {
        // SAFETY: parent is necessarily different from `self.0`
        unsafe {
            let next = (*self.0).h.index_in_parent + 1;
            Some(NodeRefMut(
                *(*self.0).h.parent.as_mut()?.h.children.get_mut(next)?,
                PhantomData,
            ))
        }
    }

    /// Moves this reference to the first child.
    pub fn first_child(self) -> Option<NodeRefMut<'a, N>> {
        // SAFETY: the first child is guaranteed to be valid as long as the parent node is alive.
        unsafe { Some(NodeRefMut(*(*self.0).h.children.first()?, PhantomData)) }
    }

    /// Returns an iterator over the children of this node.
    pub fn children(&self) -> impl Iterator<Item=&'a N> + '_ {
        // SAFETY: the children are guaranteed to be valid as long as the parent node is alive.
        unsafe { (*self.0).h.children.iter().map(|&child| &(*child).inner) }
    }

    ///
    pub fn children_mut(&mut self) -> impl Iterator<Item=&'a mut N> + '_ {
        unsafe { (*self.0).h.children.iter_mut().map(|&mut child| &mut (*child).inner) }
    }

    /// Moves this reference to the specified node.
    pub fn move_to(self, key: NodeKey) -> Option<NodeRefMut<'a, N>> {
        // SAFETY: the node is guaranteed to be valid as long as the tree is alive.
        unsafe {
            let nodes = &mut *(*self.0).h.tree;
            Some(NodeRefMut(*nodes.get_mut(key)?, PhantomData))
        }
    }

    /*/// Returns an iterator over the descendants of this node (in depth-first order).
    pub fn descendants(&self) -> impl Iterator<Item=&'a N> + '_ {
        let mut stack = VecDeque::from(self.0.children.clone());
        std::iter::from_fn(move || {
            let node = stack.pop_front()?;
            stack.extend(node.children());
            Some(node)
        })
    }*/

    /*
    /// Returns an iterator over the descendants of this node (in depth-first order).
    pub fn descendants_mut(&mut self) -> impl Iterator<Item=&'a mut N> + '_ {
        let mut stack = vec![self];
        std::iter::from_fn(move || {
            let node = stack.pop()?;
            stack.extend(node.children_mut());
            Some(node)
        })
    }*/

    /// Returns an iterator over the parents of this node.
    pub fn ancestors(&self) -> impl Iterator<Item=&'a N> + '_ {
        unsafe {
            let mut current = (*self.0).h.parent;
            std::iter::from_fn(move || {
                if current.is_null() {
                    None
                } else {
                    // SAFETY: the parent is guaranteed to be valid as long as the child node is alive.
                    let parent = &*current;
                    current = parent.h.parent;
                    Some(&parent.inner)
                }
            })
        }
    }

    pub fn ancestors_mut(&mut self) -> impl Iterator<Item=&'a mut N> + '_ {
        unsafe {
            let mut current = (*self.0).h.parent;
            std::iter::from_fn(move || {
                if current.is_null() {
                    None
                } else {
                    // SAFETY: the parent is guaranteed to be valid as long as the child node is alive.
                    let parent = &mut *current;
                    current = parent.h.parent;
                    Some(&mut parent.inner)
                }
            })
        }
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use crate::tree::{NodeRef, NodeRefMut, Tree};

    fn dump_tree_details(node: NodeRefMut<&'static str>, indent: usize) {
        let ptr = node.0 as *const _;
        eprintln!(
            "{}{} ({:p} key={:?} parent={:p} index_in_parent={})",
            "  ".repeat(indent),
            *node,
            ptr,
            node.key(),
            unsafe { (*node.0).h.parent },
            unsafe { (*node.0).h.index_in_parent }
        );
        let indent = indent + 1;
        let mut current = node.first_child();
        while let Some(mut child) = current {
            dump_tree_details(child.reborrow(), indent);
            current = child.next_sibling();
        }
    }

    fn tree(desc: &'static str) -> Tree<char> {
        let mut tree = Tree::new();
        let mut stack = vec![];
        let mut last = None;
        for p in desc.chars() {
            match p {
                '(' => {
                    stack.push(last.unwrap());
                }
                ')' => {
                    last = stack.pop();
                }
                name => {
                    let mut node = tree.create(name);
                    if let Some(parent) = stack.last() {
                        node = node.insert_as_child(*parent);
                    }
                    last = Some(node.key());
                }
            }
        }
        tree
    }

    fn dump_tree(tree: &Tree<char>) -> String {
        let mut result = String::new();
        fn dump_tree(node: NodeRef<char>, result: &mut String) {
            result.push(*node);
            if node.child_count() > 0 {
                result.push('(');
                let mut current = node.first_child();
                while let Some(child) = current {
                    dump_tree(child, result);
                    current = child.next_sibling();
                }
                result.push(')');
            }
        }
        for root in tree.orphans() {
            dump_tree(root, &mut result);
        }
        result
    }

    #[test]
    fn structure() {
        let t1 = tree("A(BCD)");
        assert_eq!(dump_tree(&t1), "A(BCD)");

        let t2 = tree("A(B(CD)E)");
        assert_eq!(dump_tree(&t2), "A(B(CD)E)");

        let t3 = tree("A(B(CD)EF(GHI)JK(LM(NO)PQ))");
        assert_eq!(dump_tree(&t3), "A(B(CD)EF(GHI)JK(LM(NO)PQ))");
    }

    #[test]
    fn back_and_forth() {
        let mut t1 = tree("A(BCD)");
        let a = t1.iter().find(|n| **n == 'A').unwrap().key();
        let b = t1.iter().find(|n| **n == 'B').unwrap().key();

        let mut b_mut = t1.get_mut(b).unwrap();
        *b_mut = 'E';
        let mut a_mut = b_mut.parent().unwrap();
        *a_mut = 'F';
        let b_mut = a_mut.first_child().unwrap();
        assert_eq!(*b_mut, 'E');
        assert_eq!(dump_tree(&t1), "F(ECD)");

        t1.remove(b);
        assert_eq!(dump_tree(&t1), "F(CD)");

        t1.remove(a);
        assert_eq!(dump_tree(&t1), "");
    }

    #[test]
    fn basic() {
        let mut tree = Tree::new();
        let root = tree.create("Root").key();
        let child_1 = tree.create("Child 1").insert_as_child(root).key();
        let child_1_1 = tree.create("Child 1.1").insert_as_child(child_1).key();
        let child_2 = tree.create("Child 2").insert_as_child(root).key();
        let child_2_1 = tree.create("Child 2.1").insert_as_child(child_2).key();
        let child_2_1_1 = tree.create("Child 2.1.1").insert_as_child(child_2_1).key();
        let child_3 = tree.create("Child 3").insert_as_child(root).key();
        let child_3_1 = tree.create("Child 3.1").insert_as_child(child_3).key();
        let child_3_2 = tree.create("Child 3.2").insert_as_child(child_3).key();
        let child_3_3 = tree.create("Child 3.3").insert_as_child(child_3).key();
        let child_3_4 = tree.create("Child 3.4").insert_as_child(child_3).key();
        let child_3_4_1 = tree.create("Child 3.4.1").insert_as_child(child_3_4).key();
        let child_4 = tree.create("Child 4").insert_as_child(root).key();

        eprintln!("== Dump: ==");
        dump_tree_details(tree.get_mut(root).unwrap(), 0);

        eprintln!("== Ancestors of Child 2.1.1: ==");
        let ancestors = tree
            .get_mut(child_2_1_1)
            .unwrap()
            .ancestors()
            .map(|a| *a)
            .collect::<Vec<_>>();
        assert_eq!(ancestors, vec!["Child 2.1", "Child 2", "Root"]);
        eprintln!("{:?}", ancestors);

        eprintln!("=== Remove Child 3 ===");
        tree.remove(child_3).unwrap();
        dump_tree_details(tree.get_mut(root).unwrap(), 0);

        eprintln!("=== Lookup Child 4 ===");
        let child_4 = tree.get_mut(child_4).unwrap();
        eprintln!("{:?}", child_4.ancestors().map(|a| *a).collect::<Vec<_>>());
    }
}
