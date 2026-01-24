//! [`TreeIndex`] is a read-optimized asynchronous/concurrent B-plus tree.

mod internal_node;
mod leaf;
mod leaf_node;
mod node;

use std::fmt::{self, Debug};
use std::iter::FusedIterator;
use std::marker::PhantomData;
use std::ops::Bound::{Excluded, Included, Unbounded};
use std::ops::RangeBounds;
use std::panic::UnwindSafe;
use std::pin::pin;
use std::sync::atomic::Ordering::{AcqRel, Acquire};

use sdd::{AtomicShared, Guard, Ptr, Shared, Tag};

use crate::Comparable;
use crate::async_helper::AsyncWait;
use leaf::Iter as LeafIter;
use leaf::RevIter as LeafRevIter;
use leaf::{InsertResult, Leaf, RemoveResult};
use node::Node;

/// Scalable asynchronous/concurrent B-plus tree.
///
/// [`TreeIndex`] is a asynchronous/concurrent B-plus tree variant optimized for read operations.
/// Read operations, such as read iteration over entries, are neither blocked nor interrupted by
/// other threads or tasks. Write operations, such as insert and remove, do not block if structural
/// changes are not required.
///
/// ## Note
///
/// [`TreeIndex`] methods are linearizable. However, its iterator methods are not; [`Iter`] and
/// [`Range`] are only guaranteed to observe events that happened before the first call to
/// [`Iterator::next`].
///
/// ## The key features of [`TreeIndex`]
///
/// * Lock-free read: read and scan operations are never blocked and do not modify shared data.
/// * Near lock-free write: write operations do not block unless a structural change is needed.
/// * No busy waiting: each node has a wait queue to avoid spinning.
/// * Immutability: the data in the container is immutable until it becomes unreachable.
///
/// ## The key statistics for [`TreeIndex`]
///
/// * The maximum number of entries that a leaf can contain: 14.
/// * The maximum number of leaves or child nodes a node can contain: 15.
///
/// ## Locking behavior
///
/// Read access is always lock-free and non-blocking. Write access to an entry is also lock-free and
/// non-blocking as long as no structural changes are required. However, when nodes are split or
/// merged by a write operation, other write operations on keys in the affected range are blocked.
///
/// ### Synchronous methods in an asynchronous code block
///
/// It is generally not recommended to use blocking methods, such as [`TreeIndex::insert_sync`], in
/// an asynchronous code block or [`poll`](std::future::Future::poll), since it may lead to
/// deadlocks or performance degradation.
///
/// ### Unwind safety
///
/// [`TreeIndex`] is impervious to out-of-memory errors and panics in user-specified code under one
/// condition; `K::drop` and `V::drop` must not panic.
pub struct TreeIndex<K, V> {
    root: AtomicShared<Node<K, V>>,
}

/// An iterator over the entries of a [`TreeIndex`].
///
/// An [`Iter`] iterates over all the entries that exist during the lifetime of the [`Iter`] in
/// monotonically increasing order.
pub struct Iter<'t, 'g, K, V> {
    root: &'t AtomicShared<Node<K, V>>,
    forward: Option<LeafIter<'g, K, V>>,
    backward: Option<LeafRevIter<'g, K, V>>,
    guard: &'g Guard,
}

/// An iterator over a sub-range of entries in a [`TreeIndex`].
pub struct Range<'t, 'g, K, V, Q: ?Sized, R: RangeBounds<Q>> {
    root: &'t AtomicShared<Node<K, V>>,
    leaf_iter: Option<LeafIter<'g, K, V>>,
    bounds: R,
    check_upper_bound: bool,
    guard: &'g Guard,
    query: PhantomData<fn() -> Q>,
}

impl<K, V> TreeIndex<K, V> {
    /// Creates an empty [`TreeIndex`].
    ///
    /// # Examples
    ///
    /// ```
    /// use scc::TreeIndex;
    ///
    /// let treeindex: TreeIndex<u64, u32> = TreeIndex::new();
    /// ```
    #[cfg(not(feature = "loom"))]
    #[inline]
    #[must_use]
    pub const fn new() -> Self {
        Self {
            root: AtomicShared::null(),
        }
    }

    /// Creates an empty [`TreeIndex`].
    #[cfg(feature = "loom")]
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self {
            root: AtomicShared::null(),
        }
    }

    /// Clears the [`TreeIndex`].
    ///
    /// # Examples
    ///
    /// ```
    /// use scc::TreeIndex;
    ///
    /// let treeindex: TreeIndex<u64, u32> = TreeIndex::new();
    ///
    /// treeindex.clear();
    /// assert_eq!(treeindex.len(), 0);
    /// ```
    #[inline]
    pub fn clear(&self) {
        if let (Some(root), _) = self.root.swap((None, Tag::None), Acquire) {
            root.clear(&Guard::new());
        }
    }

    /// Returns the depth of the [`TreeIndex`].
    ///
    /// # Examples
    ///
    /// ```
    /// use scc::TreeIndex;
    ///
    /// let treeindex: TreeIndex<u64, u32> = TreeIndex::new();
    /// assert_eq!(treeindex.depth(), 0);
    /// ```
    #[inline]
    #[must_use]
    pub fn depth(&self) -> usize {
        let guard = Guard::new();
        self.root
            .load(Acquire, &guard)
            .as_ref()
            .map_or(0, |root_ref| root_ref.depth(1, &guard))
    }
}

impl<K, V> TreeIndex<K, V>
where
    K: 'static + Clone + Ord,
    V: 'static,
{
    /// Inserts a key-value pair.
    ///
    /// # Errors
    ///
    /// Returns an error along with the supplied key-value pair if the key exists.
    ///
    /// # Examples
    ///
    /// ```
    /// use scc::TreeIndex;
    ///
    /// let treeindex: TreeIndex<u64, u32> = TreeIndex::new();
    /// let future_insert = treeindex.insert_async(1, 10);
    /// ```
    #[inline]
    pub async fn insert_async(&self, mut key: K, mut val: V) -> Result<(), (K, V)> {
        let mut pinned_async_wait = pin!(AsyncWait::default());
        loop {
            {
                let guard = Guard::new();
                let root_ptr = self.root.load(Acquire, &guard);
                if let Some(root_ref) = root_ptr.as_ref() {
                    match root_ref.insert(key, val, &mut pinned_async_wait, &guard) {
                        Ok(r) => match r {
                            InsertResult::Success => return Ok(()),
                            InsertResult::Duplicate(k, v) | InsertResult::Frozen(k, v) => {
                                return Err((k, v));
                            }
                            InsertResult::Full(k, v) => {
                                key = k;
                                val = v;
                                Node::split_root(root_ptr, &self.root, &guard);
                                continue;
                            }
                        },
                        Err((k, v)) => {
                            key = k;
                            val = v;
                        }
                    }
                } else if let Err((Some(new_node), _)) = self.root.compare_exchange(
                    Ptr::null(),
                    (Some(Shared::new(Node::new_leaf_node())), Tag::None),
                    AcqRel,
                    Acquire,
                    &Guard::new(),
                ) {
                    unsafe {
                        let _: bool = new_node.drop_in_place();
                    }
                    continue;
                }
            };
            pinned_async_wait.wait().await;
        }
    }

    /// Inserts a key-value pair.
    ///
    /// # Errors
    ///
    /// Returns an error along with the supplied key-value pair if the key exists.
    ///
    /// # Examples
    ///
    /// ```
    /// use scc::TreeIndex;
    ///
    /// let treeindex: TreeIndex<u64, u32> = TreeIndex::new();
    ///
    /// assert!(treeindex.insert_sync(1, 10).is_ok());
    /// assert_eq!(treeindex.insert_sync(1, 11).err().unwrap(), (1, 11));
    /// assert_eq!(treeindex.peek_with(&1, |k, v| *v).unwrap(), 10);
    /// ```
    #[inline]
    pub fn insert_sync(&self, mut key: K, mut val: V) -> Result<(), (K, V)> {
        loop {
            let guard = Guard::new();
            let root_ptr = self.root.load(Acquire, &guard);
            if let Some(root_ref) = root_ptr.as_ref() {
                match root_ref.insert(key, val, &mut (), &guard) {
                    Ok(r) => match r {
                        InsertResult::Success => return Ok(()),
                        InsertResult::Duplicate(k, v) | InsertResult::Frozen(k, v) => {
                            return Err((k, v));
                        }
                        InsertResult::Full(k, v) => {
                            key = k;
                            val = v;
                            Node::split_root(root_ptr, &self.root, &guard);
                        }
                    },
                    Err((k, v)) => {
                        key = k;
                        val = v;
                    }
                }
            } else if let Err((Some(new_node), _)) = self.root.compare_exchange(
                Ptr::null(),
                (Some(Shared::new(Node::new_leaf_node())), Tag::None),
                AcqRel,
                Acquire,
                &Guard::new(),
            ) {
                unsafe {
                    let _: bool = new_node.drop_in_place();
                }
            }
        }
    }

    /// Removes a key-value pair.
    ///
    /// Returns `false` if the key does not exist. Returns `true` if the key existed and the
    /// condition was met after marking the entry unreachable; the memory will be reclaimed later.
    ///
    /// # Examples
    ///
    /// ```
    /// use scc::TreeIndex;
    ///
    /// let treeindex: TreeIndex<u64, u32> = TreeIndex::new();
    /// let future_remove = treeindex.remove_async(&1);
    /// ```
    #[inline]
    pub async fn remove_async<Q>(&self, key: &Q) -> bool
    where
        Q: Comparable<K> + ?Sized,
    {
        self.remove_if_async(key, |_| true).await
    }

    /// Removes a key-value pair.
    ///
    /// Returns `false` if the key does not exist.
    ///
    /// Returns `true` if the key existed and the condition was met after marking the entry
    /// unreachable; the memory will be reclaimed later.
    ///
    /// # Examples
    ///
    /// ```
    /// use scc::TreeIndex;
    ///
    /// let treeindex: TreeIndex<u64, u32> = TreeIndex::new();
    ///
    /// assert!(!treeindex.remove_sync(&1));
    /// assert!(treeindex.insert_sync(1, 10).is_ok());
    /// assert!(treeindex.remove_sync(&1));
    /// ```
    #[inline]
    pub fn remove_sync<Q>(&self, key: &Q) -> bool
    where
        Q: Comparable<K> + ?Sized,
    {
        self.remove_if_sync(key, |_| true)
    }

    /// Removes a key-value pair if the given condition is met.
    ///
    /// Returns `false` if the key does not exist or the condition was not met. Returns `true` if
    /// the key existed and the condition was met after marking the entry unreachable; the memory
    /// will be reclaimed later.
    ///
    /// # Examples
    ///
    /// ```
    /// use scc::TreeIndex;
    ///
    /// let treeindex: TreeIndex<u64, u32> = TreeIndex::new();
    /// let future_remove = treeindex.remove_if_async(&1, |v| *v == 0);
    /// ```
    #[inline]
    pub async fn remove_if_async<Q, F: FnMut(&V) -> bool>(&self, key: &Q, mut condition: F) -> bool
    where
        Q: Comparable<K> + ?Sized,
    {
        let mut pinned_async_wait = pin!(AsyncWait::default());
        let mut removed = false;
        loop {
            {
                let guard = Guard::new();
                if let Some(root_ref) = self.root.load(Acquire, &guard).as_ref() {
                    if let Ok(result) = root_ref.remove_if::<_, _, _>(
                        key,
                        &mut condition,
                        &mut pinned_async_wait,
                        &guard,
                    ) {
                        match result {
                            RemoveResult::Success => return true,
                            RemoveResult::Retired => {
                                if Node::cleanup_root(&self.root, &mut pinned_async_wait, &guard) {
                                    return true;
                                }
                                removed = true;
                            }
                            RemoveResult::Fail => {
                                if removed {
                                    if Node::cleanup_root(
                                        &self.root,
                                        &mut pinned_async_wait,
                                        &guard,
                                    ) {
                                        return true;
                                    }
                                } else {
                                    return false;
                                }
                            }
                            RemoveResult::Frozen => (),
                        }
                    }
                } else {
                    return removed;
                }
            }
            pinned_async_wait.wait().await;
        }
    }

    /// Removes a key-value pair if the given condition is met.
    ///
    /// Returns `false` if the key does not exist or the condition was not met.
    ///
    /// Returns `true` if the key existed and the condition was met after marking the entry
    /// unreachable; the memory will be reclaimed later.
    ///
    /// # Examples
    ///
    /// ```
    /// use scc::TreeIndex;
    ///
    /// let treeindex: TreeIndex<u64, u32> = TreeIndex::new();
    ///
    /// assert!(treeindex.insert_sync(1, 10).is_ok());
    /// assert!(!treeindex.remove_if_sync(&1, |v| *v == 0));
    /// assert!(treeindex.remove_if_sync(&1, |v| *v == 10));
    /// ```
    #[inline]
    pub fn remove_if_sync<Q, F: FnMut(&V) -> bool>(&self, key: &Q, mut condition: F) -> bool
    where
        Q: Comparable<K> + ?Sized,
    {
        let mut removed = false;
        loop {
            let guard = Guard::new();
            if let Some(root_ref) = self.root.load(Acquire, &guard).as_ref() {
                if let Ok(result) =
                    root_ref.remove_if::<_, _, _>(key, &mut condition, &mut (), &guard)
                {
                    match result {
                        RemoveResult::Success => return true,
                        RemoveResult::Retired => {
                            if Node::cleanup_root(&self.root, &mut (), &guard) {
                                return true;
                            }
                            removed = true;
                        }
                        RemoveResult::Fail => {
                            if removed {
                                if Node::cleanup_root(&self.root, &mut (), &guard) {
                                    return true;
                                }
                            } else {
                                return false;
                            }
                        }
                        RemoveResult::Frozen => (),
                    }
                }
            } else {
                return removed;
            }
        }
    }

    /// Removes keys in the specified range.
    ///
    /// This method removes internal nodes that are definitely contained in the specified range
    /// first, and then removes remaining entries individually.
    ///
    /// # Note
    ///
    /// Internally, multiple internal node locks need to be acquired, thus making this method
    /// susceptible to lock starvation.
    ///
    /// # Examples
    ///
    /// ```
    /// use scc::TreeIndex;
    ///
    /// let treeindex: TreeIndex<u64, u32> = TreeIndex::new();
    ///
    /// for k in 2..8 {
    ///     assert!(treeindex.insert_sync(k, 1).is_ok());
    /// }
    ///
    /// let future_remove_range = treeindex.remove_range_async(3..8);
    /// ```
    #[inline]
    pub async fn remove_range_async<Q, R: RangeBounds<Q>>(&self, range: R)
    where
        Q: Comparable<K> + ?Sized,
    {
        let mut pinned_async_wait = pin!(AsyncWait::default());
        let start_unbounded = matches!(range.start_bound(), Unbounded);

        loop {
            {
                let guard = Guard::new();

                // Remove internal nodes, and individual entries in affected leaves.
                //
                // It takes O(N) to traverse sub-trees on the range border.
                if let Some(root_ref) = self.root.load(Acquire, &guard).as_ref() {
                    if let Ok(num_children) = root_ref.remove_range(
                        &range,
                        start_unbounded,
                        None,
                        None,
                        &mut pinned_async_wait,
                        &guard,
                    ) {
                        if num_children >= 2
                            || Node::cleanup_root(&self.root, &mut pinned_async_wait, &guard)
                        {
                            // Completed removal and cleaning up the root.
                            return;
                        }
                    }
                } else {
                    // Nothing to remove.
                    return;
                }
            }
            pinned_async_wait.wait().await;
        }
    }

    /// Removes keys in the specified range.
    ///
    /// This method removes internal nodes that are definitely contained in the specified range
    /// first, and then removes remaining entries individually.
    ///
    /// # Note
    ///
    /// Internally, multiple internal node locks need to be acquired, thus making this method
    /// susceptible to lock starvation.
    ///
    /// # Examples
    ///
    /// ```
    /// use scc::TreeIndex;
    ///
    /// let treeindex: TreeIndex<u64, u32> = TreeIndex::new();
    ///
    /// for k in 2..8 {
    ///     assert!(treeindex.insert_sync(k, 1).is_ok());
    /// }
    ///
    /// treeindex.remove_range_sync(3..8);
    ///
    /// assert!(treeindex.contains(&2));
    /// assert!(!treeindex.contains(&3));
    /// ```
    #[inline]
    pub fn remove_range_sync<Q, R: RangeBounds<Q>>(&self, range: R)
    where
        Q: Comparable<K> + ?Sized,
    {
        let start_unbounded = matches!(range.start_bound(), Unbounded);
        let guard = Guard::new();

        // Remove internal nodes, and individual entries in affected leaves.
        //
        // It takes O(N) to traverse sub-trees on the range border.
        while let Some(root_ref) = self.root.load(Acquire, &guard).as_ref() {
            if let Ok(num_children) =
                root_ref.remove_range(&range, start_unbounded, None, None, &mut (), &guard)
            {
                if num_children < 2 && !Node::cleanup_root(&self.root, &mut (), &guard) {
                    continue;
                }
                break;
            }
        }
    }

    /// Returns a guarded reference to the value for the specified key without acquiring locks.
    ///
    /// Returns `None` if the key does not exist. The returned reference can survive as long as the
    /// associated [`Guard`] is alive.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::sync::Arc;
    ///
    /// use scc::TreeIndex;
    ///
    /// use sdd::Guard;
    ///
    /// let treeindex: TreeIndex<Arc<str>, u32> = TreeIndex::new();
    ///
    /// let guard = Guard::new();
    /// assert!(treeindex.peek("foo", &guard).is_none());
    ///
    /// treeindex.insert_sync("foo".into(), 1).expect("insert in empty TreeIndex");
    /// ```
    #[inline]
    pub fn peek<'g, Q>(&self, key: &Q, guard: &'g Guard) -> Option<&'g V>
    where
        Q: Comparable<K> + ?Sized,
    {
        if let Some(root_ref) = self.root.load(Acquire, guard).as_ref() {
            return root_ref.search_value(key, guard);
        }
        None
    }

    /// Peeks a key-value pair without acquiring locks.
    ///
    /// Returns `None` if the key does not exist.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::sync::Arc;
    /// use scc::TreeIndex;
    ///
    /// let treeindex: TreeIndex<Arc<str>, u32> = TreeIndex::new();
    ///
    /// assert!(treeindex.peek_with("foo", |k, v| *v).is_none());
    ///
    /// treeindex.insert_sync("foo".into(), 1).expect("insert in empty TreeIndex");
    ///
    /// let key: Arc<str> = treeindex
    ///     .peek_with("foo", |k, _v| Arc::clone(k))
    ///     .expect("peek_with by borrowed key");
    /// ```
    #[inline]
    pub fn peek_with<Q, R, F: FnOnce(&K, &V) -> R>(&self, key: &Q, reader: F) -> Option<R>
    where
        Q: Comparable<K> + ?Sized,
    {
        let guard = Guard::new();
        self.peek_entry(key, &guard).map(|(k, v)| reader(k, v))
    }

    /// Returns a guarded reference to the key-value pair for the specified key without acquiring locks.
    ///
    /// Returns `None` if the key does not exist. The returned reference can survive as long as the
    /// associated [`Guard`] is alive.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::sync::Arc;
    ///
    /// use scc::TreeIndex;
    ///
    /// use sdd::Guard;
    ///
    /// let treeindex: TreeIndex<Arc<str>, u32> = TreeIndex::new();
    ///
    /// let guard = Guard::new();
    /// assert!(treeindex.peek_entry("foo", &guard).is_none());
    ///
    /// treeindex.insert_sync("foo".into(), 1).expect("insert in empty TreeIndex");
    ///
    /// let key: Arc<str> = treeindex
    ///     .peek_entry("foo", &guard)
    ///     .map(|(k, _v)| Arc::clone(k))
    ///     .expect("peek_entry by borrowed key");
    /// ```
    #[inline]
    pub fn peek_entry<'g, Q>(&self, key: &Q, guard: &'g Guard) -> Option<(&'g K, &'g V)>
    where
        Q: Comparable<K> + ?Sized,
    {
        if let Some(root_ref) = self.root.load(Acquire, guard).as_ref() {
            return root_ref.search_entry(key, guard);
        }
        None
    }

    /// Returns `true` if the [`TreeIndex`] contains the key.
    ///
    /// # Examples
    ///
    /// ```
    /// use scc::TreeIndex;
    ///
    /// let treeindex: TreeIndex<u64, u32> = TreeIndex::default();
    ///
    /// assert!(!treeindex.contains(&1));
    /// assert!(treeindex.insert_sync(1, 0).is_ok());
    /// assert!(treeindex.contains(&1));
    /// ```
    #[inline]
    pub fn contains<Q>(&self, key: &Q) -> bool
    where
        Q: Comparable<K> + ?Sized,
    {
        self.peek(key, &Guard::new()).is_some()
    }

    /// Returns the size of the [`TreeIndex`].
    ///
    /// It internally scans all the leaf nodes, and therefore the time complexity is O(N).
    ///
    /// # Examples
    ///
    /// ```
    /// use scc::TreeIndex;
    ///
    /// let treeindex: TreeIndex<u64, u32> = TreeIndex::new();
    /// assert_eq!(treeindex.len(), 0);
    /// ```
    #[inline]
    pub fn len(&self) -> usize {
        let guard = Guard::new();
        self.iter(&guard).count()
    }

    /// Returns `true` if the [`TreeIndex`] is empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use scc::TreeIndex;
    ///
    /// let treeindex: TreeIndex<u64, u32> = TreeIndex::new();
    ///
    /// assert!(treeindex.is_empty());
    /// ```
    #[inline]
    pub fn is_empty(&self) -> bool {
        let guard = Guard::new();
        !self.iter(&guard).any(|_| true)
    }

    /// Returns an [`Iter`].
    ///
    /// The returned [`Iter`] is a [`DoubleEndedIterator`] that allows scanning in both ascending
    /// and descending order. [`Iter`] may miss newly inserted key-value pairs after the invocation
    /// of this method, because [`Self::iter`] is the linearization point whereas [`Iter::next`] and
    /// [`Iter::next_back`] are not.
    ///
    /// # Examples
    ///
    /// ```
    /// use scc::TreeIndex;
    ///
    /// use sdd::Guard;
    ///
    /// let treeindex: TreeIndex<u64, u32> = TreeIndex::new();
    ///
    /// assert!(treeindex.insert_sync(1, 2).is_ok());
    /// assert!(treeindex.insert_sync(3, 4).is_ok());
    ///
    /// let guard = Guard::new();
    /// let mut iter = treeindex.iter(&guard);
    /// assert_eq!(iter.next(), Some((&1, &2)));
    /// assert_eq!(iter.next_back(), Some((&3, &4)));
    /// assert_eq!(iter.next(), None);
    /// assert_eq!(iter.next_back(), None);
    /// ```
    #[inline]
    pub const fn iter<'t, 'g>(&'t self, guard: &'g Guard) -> Iter<'t, 'g, K, V> {
        Iter::new(&self.root, guard)
    }

    /// Returns a [`Range`] that scans keys in the given range.
    ///
    /// Key-value pairs in the range are scanned in ascending order, and key-value pairs that have
    /// existed since the invocation of the method are guaranteed to be visited if they are not
    /// removed. However, it is possible to visit removed key-value pairs momentarily.
    ///
    /// # Examples
    ///
    /// ```
    /// use scc::TreeIndex;
    ///
    /// use sdd::Guard;
    ///
    /// let treeindex: TreeIndex<u64, u32> = TreeIndex::new();
    ///
    /// let guard = Guard::new();
    /// assert_eq!(treeindex.range(4..=8, &guard).count(), 0);
    /// ```
    #[inline]
    pub const fn range<'t, 'g, Q, R: RangeBounds<Q>>(
        &'t self,
        range: R,
        guard: &'g Guard,
    ) -> Range<'t, 'g, K, V, Q, R>
    where
        Q: Comparable<K> + ?Sized,
    {
        Range::new(&self.root, range, guard)
    }
}

impl<K, V> Clone for TreeIndex<K, V>
where
    K: 'static + Clone + Ord,
    V: 'static + Clone,
{
    #[inline]
    fn clone(&self) -> Self {
        let self_clone = Self::default();
        for (k, v) in self.iter(&Guard::new()) {
            let _result: Result<(), (K, V)> = self_clone.insert_sync(k.clone(), v.clone());
        }
        self_clone
    }
}

impl<K, V> Debug for TreeIndex<K, V>
where
    K: 'static + Clone + Debug + Ord,
    V: 'static + Debug,
{
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let guard = Guard::new();
        f.debug_map().entries(self.iter(&guard)).finish()
    }
}

impl<K, V> Default for TreeIndex<K, V> {
    /// Creates a [`TreeIndex`] with the default parameters.
    ///
    /// # Examples
    ///
    /// ```
    /// use scc::TreeIndex;
    ///
    /// let treeindex: TreeIndex<u64, u32> = TreeIndex::default();
    /// ```
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl<K, V> Drop for TreeIndex<K, V> {
    #[inline]
    fn drop(&mut self) {
        self.clear();
    }
}

impl<K, V> PartialEq for TreeIndex<K, V>
where
    K: 'static + Clone + Ord,
    V: 'static + PartialEq,
{
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        // The key order is preserved, therefore comparing iterators suffices.
        let guard = Guard::new();
        Iterator::eq(self.iter(&guard), other.iter(&guard))
    }
}

impl<K, V> UnwindSafe for TreeIndex<K, V> {}

impl<'t, 'g, K, V> Iter<'t, 'g, K, V> {
    #[inline]
    const fn new(root: &'t AtomicShared<Node<K, V>>, guard: &'g Guard) -> Iter<'t, 'g, K, V> {
        Iter::<'t, 'g, K, V> {
            root,
            forward: None,
            backward: None,
            guard,
        }
    }
}

impl<'g, K, V> Iter<'_, 'g, K, V>
where
    K: Ord,
{
    fn check_collision<const FORWARD: bool>(
        &mut self,
        entry: (&'g K, &'g V),
    ) -> Option<(&'g K, &'g V)> {
        let other_entry = if FORWARD {
            self.backward.as_ref().and_then(LeafRevIter::get)
        } else {
            self.forward.as_ref().and_then(LeafIter::get)
        };
        let Some(other_entry) = other_entry else {
            // The other iterator was exhausted.
            return None;
        };
        if (FORWARD && other_entry.0 > entry.0) || (!FORWARD && other_entry.0 < entry.0) {
            return Some(entry);
        }
        None
    }
}

impl<K, V> Debug for Iter<'_, '_, K, V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Iter")
            .field("root", &self.root)
            .field("leaf_iter", &self.forward)
            .finish()
    }
}

impl<K, V> DoubleEndedIterator for Iter<'_, '_, K, V>
where
    K: 'static + Clone + Ord,
    V: 'static,
{
    #[inline]
    fn next_back(&mut self) -> Option<Self::Item> {
        // Start iteration.
        if self.backward.is_none() {
            let root_ptr = self.root.load(Acquire, self.guard);
            if let Some(root_ref) = root_ptr.as_ref() {
                if let Some(rev_iter) = root_ref.max(self.guard) {
                    self.backward.replace(rev_iter);
                }
            } else {
                return None;
            }
        }

        // Go to the prev entry.
        if let Some(rev_iter) = self.backward.as_mut() {
            if let Some(entry) = rev_iter.next() {
                if self.forward.is_some() {
                    return self.check_collision::<false>(entry);
                }
                return Some(entry);
            }
            // Go to the prev leaf node.
            if let Some(new_rev_iter) = rev_iter.jump(self.guard) {
                if let Some(entry) = new_rev_iter.get() {
                    self.backward.replace(new_rev_iter);
                    if self.forward.is_some() {
                        return self.check_collision::<false>(entry);
                    }
                    return Some(entry);
                }
            }
        }

        None
    }
}

impl<'g, K, V> Iterator for Iter<'_, 'g, K, V>
where
    K: 'static + Clone + Ord,
    V: 'static,
{
    type Item = (&'g K, &'g V);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        // Start iteration.
        if self.forward.is_none() {
            let root_ptr = self.root.load(Acquire, self.guard);
            if let Some(root_ref) = root_ptr.as_ref() {
                if let Some(iter) = root_ref.min(self.guard) {
                    self.forward.replace(iter);
                }
            } else {
                return None;
            }
        }

        // Go to the next entry.
        if let Some(iter) = self.forward.as_mut() {
            if let Some(entry) = iter.next() {
                if self.backward.is_some() {
                    return self.check_collision::<true>(entry);
                }
                return Some(entry);
            }
            // Go to the next leaf node.
            if let Some(new_iter) = iter.jump(self.guard) {
                if let Some(entry) = new_iter.get() {
                    self.forward.replace(new_iter);
                    if self.backward.is_some() {
                        return self.check_collision::<true>(entry);
                    }
                    return Some(entry);
                }
            }
        }

        None
    }
}

impl<K, V> FusedIterator for Iter<'_, '_, K, V>
where
    K: 'static + Clone + Ord,
    V: 'static,
{
}

impl<K, V> UnwindSafe for Iter<'_, '_, K, V> {}

impl<'t, 'g, K, V, Q: ?Sized, R: RangeBounds<Q>> Range<'t, 'g, K, V, Q, R> {
    #[inline]
    const fn new(
        root: &'t AtomicShared<Node<K, V>>,
        range: R,
        guard: &'g Guard,
    ) -> Range<'t, 'g, K, V, Q, R> {
        Range::<'t, 'g, K, V, Q, R> {
            root,
            leaf_iter: None,
            bounds: range,
            check_upper_bound: false,
            guard,
            query: PhantomData,
        }
    }
}

impl<'g, K, V, Q, R> Range<'_, 'g, K, V, Q, R>
where
    K: 'static + Clone + Ord,
    V: 'static,
    Q: Comparable<K> + ?Sized,
    R: RangeBounds<Q>,
{
    fn start(&mut self) -> Option<(&'g K, &'g V)> {
        // Start iteration.
        let root_ptr = self.root.load(Acquire, self.guard);
        if let Some(root) = root_ptr.as_ref() {
            let mut leaf_iter = match self.bounds.start_bound() {
                Excluded(k) | Included(k) => root.approximate::<_, true>(k, self.guard),
                Unbounded => None,
            };
            if leaf_iter.is_none() {
                if let Some(mut iter) = root.min(self.guard) {
                    iter.next();
                    leaf_iter.replace(iter);
                }
            }
            if let Some(mut leaf_iter) = leaf_iter {
                while let Some((k, v)) = leaf_iter.get() {
                    let check_failed = match self.bounds.start_bound() {
                        Excluded(key) => key.compare(k).is_ge(),
                        Included(key) => key.compare(k).is_gt(),
                        Unbounded => false,
                    };
                    if check_failed {
                        if leaf_iter.next().is_none() {
                            leaf_iter = leaf_iter.jump(self.guard)?;
                        }
                        continue;
                    }

                    self.set_check_upper_bound(&leaf_iter);
                    self.leaf_iter.replace(leaf_iter);
                    return Some((k, v));
                }
            }
        }
        None
    }

    #[inline]
    fn next_unbounded(&mut self) -> Option<(&'g K, &'g V)> {
        if self.leaf_iter.is_none() {
            return self.start();
        }

        // Go to the next entry.
        if let Some(leaf_iter) = self.leaf_iter.as_mut() {
            if let Some(result) = leaf_iter.next() {
                return Some(result);
            }
            // Go to the next leaf node.
            if let Some(new_iter) = leaf_iter.jump(self.guard) {
                if let Some(entry) = new_iter.get() {
                    self.set_check_upper_bound(&new_iter);
                    self.leaf_iter.replace(new_iter);
                    return Some(entry);
                }
            }
        }

        None
    }

    #[inline]
    fn set_check_upper_bound(&mut self, leaf_iter: &LeafIter<K, V>) {
        self.check_upper_bound = match self.bounds.end_bound() {
            Excluded(key) => leaf_iter.max_key().is_some_and(|k| key.compare(k).is_le()),
            Included(key) => leaf_iter.max_key().is_some_and(|k| key.compare(k).is_lt()),
            Unbounded => false,
        };
    }
}

impl<K, V, Q: ?Sized, R: RangeBounds<Q>> Debug for Range<'_, '_, K, V, Q, R> {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Range")
            .field("root", &self.root)
            .field("leaf_iter", &self.leaf_iter)
            .field("check_upper_bound", &self.check_upper_bound)
            .finish()
    }
}

impl<'g, K, V, Q, R> Iterator for Range<'_, 'g, K, V, Q, R>
where
    K: 'static + Clone + Ord,
    V: 'static,
    Q: Comparable<K> + ?Sized,
    R: RangeBounds<Q>,
{
    type Item = (&'g K, &'g V);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if let Some((k, v)) = self.next_unbounded() {
            if self.check_upper_bound {
                match self.bounds.end_bound() {
                    Excluded(key) => {
                        if key.compare(k).is_gt() {
                            return Some((k, v));
                        }
                    }
                    Included(key) => {
                        if key.compare(k).is_ge() {
                            return Some((k, v));
                        }
                    }
                    Unbounded => {
                        return Some((k, v));
                    }
                }
            } else {
                return Some((k, v));
            }
        }
        None
    }
}

impl<K, V, Q, R> FusedIterator for Range<'_, '_, K, V, Q, R>
where
    K: 'static + Clone + Ord,
    V: 'static,
    Q: Comparable<K> + ?Sized,
    R: RangeBounds<Q>,
{
}

impl<K, V, Q, R> UnwindSafe for Range<'_, '_, K, V, Q, R>
where
    Q: ?Sized,
    R: RangeBounds<Q> + UnwindSafe,
{
}
