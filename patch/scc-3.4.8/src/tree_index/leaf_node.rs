use std::cmp::Ordering::{Equal, Greater, Less};
use std::mem::forget;
use std::ops::{Bound, RangeBounds};
use std::ptr;
use std::sync::atomic::Ordering::{AcqRel, Acquire, Relaxed, Release};

use saa::Lock;
use sdd::{AtomicShared, Guard, Ptr, Shared, Tag};

use super::Leaf;
use super::leaf::{InsertResult, Iter, RemoveResult, RevIter, range_contains};
use super::node::Node;
use crate::Comparable;
use crate::async_helper::TryWait;
use crate::exit_guard::ExitGuard;

/// [`LeafNode`] contains a list of instances of `K, V` [`Leaf`].
///
/// The layout of a leaf node: `|ptr(entry array)/max(child keys)|...|ptr(entry array)|`
pub struct LeafNode<K, V> {
    /// Children of the [`LeafNode`].
    pub(super) children: Leaf<K, AtomicShared<Leaf<K, V>>>,
    /// A child [`Leaf`] that has no upper key bound.
    ///
    /// It stores the maximum key in the node, and key-value pairs are first pushed to this
    /// [`Leaf`] until it splits.
    pub(super) unbounded_child: AtomicShared<Leaf<K, V>>,
    /// [`Lock`] to protect the [`LeafNode`].
    pub(super) lock: Lock,
}

/// [`Locker`] holds exclusive ownership of a [`LeafNode`].
pub(super) struct Locker<'n, K, V> {
    leaf_node: &'n LeafNode<K, V>,
}

/// A state machine to keep track of the progress of a bulk removal operation.
#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum RemoveRangeState {
    /// The maximum key of the node is less than the start bound of the range.
    Below,
    /// The maximum key of the node is contained in the range, but it is not clear whether the
    /// minimum key of the node is contained in the range.
    MaybeBelow,
    /// The maximum key and the minimum key of the node are contained in the range.
    FullyContained,
    /// The maximum key of the node is not contained in the range, but it is not clear whether
    /// the minimum key of the node is contained in the range.
    MaybeAbove,
}

impl<K, V> LeafNode<K, V> {
    /// Creates a new empty [`LeafNode`].
    #[inline]
    pub(super) fn new() -> LeafNode<K, V> {
        LeafNode {
            children: Leaf::new(),
            unbounded_child: AtomicShared::null(),
            lock: Lock::default(),
        }
    }

    /// Clears the leaf node by unlinking all the leaves.
    #[inline]
    pub(super) fn clear(&self, guard: &Guard) {
        // Mark the unbounded to prevent any on-going split operation to cleanup itself.
        self.unbounded_child
            .update_tag_if(Tag::First, |_| true, Release, Relaxed);

        // Unlink all the children
        let iter = Iter::new(&self.children);
        for (_, child) in iter {
            let child_ptr = child.load(Acquire, guard);
            if let Some(child) = child_ptr.as_ref() {
                child.unlink(guard);
            }
        }
        let unbounded_ptr = self.unbounded_child.load(Acquire, guard);
        if let Some(unbounded) = unbounded_ptr.as_ref() {
            unbounded.unlink(guard);
        }
    }

    /// Returns `true` if the [`LeafNode`] has retired.
    #[inline]
    pub(super) fn is_retired(&self) -> bool {
        self.lock.is_poisoned(Acquire)
    }
}

impl<K, V> LeafNode<K, V>
where
    K: 'static + Clone + Ord,
    V: 'static,
{
    /// Searches for an entry containing the specified key.
    #[inline]
    pub(super) fn search_entry<'g, Q>(&self, key: &Q, guard: &'g Guard) -> Option<(&'g K, &'g V)>
    where
        K: 'g,
        Q: Comparable<K> + ?Sized,
    {
        loop {
            let (child, metadata) = self.children.min_greater_equal(key);
            if let Some((_, child)) = child {
                if let Some(child) = child.load(Acquire, guard).as_ref() {
                    if self.children.validate(metadata) {
                        // Data race with split.
                        //  - Writer: start to insert an intermediate low key leaf.
                        //  - Reader: read the metadata not including the intermediate low key leaf.
                        //  - Writer: insert the intermediate low key leaf.
                        //  - Writer: replace the high key leaf pointer.
                        //  - Reader: read the new high key leaf pointer
                        // Consequently, the reader may miss keys in the low key leaf.
                        //
                        // Resolution: metadata validation.
                        return child.search_entry(key);
                    }
                }

                // The child leaf must have been just removed.
                //
                // The `LeafNode` metadata is updated before a leaf is removed. This implies that
                // the new `metadata` will be different from the current `metadata`.
            } else {
                let unbounded_ptr = self.unbounded_child.load(Acquire, guard);
                if let Some(unbounded) = unbounded_ptr.as_ref() {
                    if self.children.validate(metadata) {
                        return unbounded.search_entry(key);
                    }
                } else {
                    return None;
                }
            }
        }
    }

    /// Searches for the value associated with the specified key.
    #[inline]
    pub(super) fn search_value<'g, Q>(&self, key: &Q, guard: &'g Guard) -> Option<&'g V>
    where
        K: 'g,
        Q: Comparable<K> + ?Sized,
    {
        loop {
            let (child, metadata) = self.children.min_greater_equal(key);
            if let Some((_, child)) = child {
                if let Some(child) = child.load(Acquire, guard).as_ref() {
                    if self.children.validate(metadata) {
                        // Data race resolution - see `LeafNode::search_entry`.
                        return child.search_value(key);
                    }
                }
                // Data race resolution - see `LeafNode::search_entry`.
            } else {
                let unbounded_ptr = self.unbounded_child.load(Acquire, guard);
                if let Some(unbounded) = unbounded_ptr.as_ref() {
                    if self.children.validate(metadata) {
                        return unbounded.search_value(key);
                    }
                } else {
                    return None;
                }
            }
        }
    }

    /// Returns the minimum key entry in the entire tree.
    #[inline]
    pub(super) fn min<'g>(&self, guard: &'g Guard) -> Option<Iter<'g, K, V>> {
        let mut min_leaf = None;
        for (_, child) in Iter::new(&self.children) {
            let child_ptr = child.load(Acquire, guard);
            if let Some(child) = child_ptr.as_ref() {
                min_leaf.replace(child);
                break;
            }
        }
        if min_leaf.is_none() {
            let unbounded_ptr = self.unbounded_child.load(Acquire, guard);
            if let Some(unbounded) = unbounded_ptr.as_ref() {
                min_leaf.replace(unbounded);
            }
        }

        let Some(min_leaf) = min_leaf else {
            // `unbounded_child` being null means that the leaf was retired of empty.
            return None;
        };

        let mut rev_iter = RevIter::new(min_leaf);
        while let Some(next_rev_iter) = rev_iter.jump(guard) {
            rev_iter = next_rev_iter;
        }
        rev_iter.rewind();
        Some(rev_iter.rev())
    }

    /// Returns the maximum key entry in the entire tree.
    #[inline]
    pub(super) fn max<'g>(&self, guard: &'g Guard) -> Option<RevIter<'g, K, V>> {
        let unbounded_ptr = self.unbounded_child.load(Acquire, guard);
        if let Some(unbounded) = unbounded_ptr.as_ref() {
            let mut iter = Iter::new(unbounded);
            while let Some(next_iter) = iter.jump(guard) {
                iter = next_iter;
                iter.rewind();
            }
            return Some(iter.rev());
        }
        // `unbounded_child` being null means that the leaf was retired of empty.
        None
    }

    /// Returns a [`Iter`] pointing to an entry that is close enough to the specified key.
    #[inline]
    pub(super) fn approximate<'g, Q, const LE: bool>(
        &self,
        key: &Q,
        guard: &'g Guard,
    ) -> Option<Iter<'g, K, V>>
    where
        K: 'g,
        Q: Comparable<K> + ?Sized,
    {
        let leaf = loop {
            if let (Some((_, child)), _) = self.children.min_greater_equal(key) {
                if let Some(child) = child.load(Acquire, guard).as_ref() {
                    break child;
                }
                // It is not a hot loop - see `LeafNode::search_entry`.
                continue;
            }
            if let Some(unbounded) = self.unbounded_child.load(Acquire, guard).as_ref() {
                break unbounded;
            }
            // `unbounded_child` being null means that the leaf was retired of empty.
            return None;
        };

        // Tries to find "any" leaf that contains a reachable entry.
        let origin = Iter::new(leaf);
        let mut iter = origin.clone();
        if iter.next().is_none() {
            if let Some(next) = iter.jump(guard) {
                iter = next;
            } else if let Some(prev) = origin.rev().jump(guard) {
                iter = prev.rev();
            } else {
                return None;
            }
        }
        iter.rewind();

        if LE {
            while let Some((k, _)) = iter.next() {
                if let Equal | Greater = key.compare(k) {
                    return Some(iter);
                }
                // Go to the prev leaf node that shall contain smaller keys.
                iter = iter.rev().jump(guard)?.rev();
                // Rewind the iterator to point to the smallest key in the leaf.
                iter.rewind();
            }
        } else {
            let mut rev_iter = iter.rev();
            while let Some((k, _)) = rev_iter.next() {
                if let Less | Equal = key.compare(k) {
                    return Some(rev_iter.rev());
                }
                // Go to the next leaf node that shall contain larger keys.
                rev_iter = rev_iter.rev().jump(guard)?.rev();
                // Rewind the iterator to point to the largest key in the leaf.
                rev_iter.rewind();
            }
        }

        // Reached the end of the linked list.
        None
    }

    /// Inserts a key-value pair.
    ///
    /// # Errors
    ///
    /// Returns an error if a retry is required.
    #[inline]
    pub(super) fn insert<W: TryWait>(
        &self,
        mut key: K,
        mut val: V,
        async_wait: &mut W,
        guard: &Guard,
    ) -> Result<InsertResult<K, V>, (K, V)> {
        loop {
            let (child, metadata) = self.children.min_greater_equal(&key);
            if let Some((_, child)) = child {
                let child_ptr = child.load(Acquire, guard);
                if let Some(child_ref) = child_ptr.as_ref() {
                    if self.children.validate(metadata) {
                        // Data race resolution - see `LeafNode::search_entry`.
                        let insert_result = child_ref.insert(key, val);
                        match insert_result {
                            InsertResult::Success | InsertResult::Duplicate(..) => {
                                return Ok(insert_result);
                            }
                            InsertResult::Full(k, v) => {
                                match self.split_leaf(child_ptr, child, async_wait, guard) {
                                    Ok(true) => {
                                        key = k;
                                        val = v;
                                        continue;
                                    }
                                    Ok(false) => return Ok(InsertResult::Full(k, v)),
                                    Err(()) => return Err((k, v)),
                                }
                            }
                            InsertResult::Frozen(k, v) => {
                                // The `Leaf` is being split: retry.
                                async_wait.try_wait(&self.lock);
                                return Err((k, v));
                            }
                        };
                    }
                }
                // It is not a hot loop - see `LeafNode::search_entry`.
                continue;
            }

            let mut unbounded_ptr = self.unbounded_child.load(Acquire, guard);
            if unbounded_ptr.is_null() {
                match self.unbounded_child.compare_exchange(
                    Ptr::null(),
                    (Some(Shared::new(Leaf::new())), Tag::None),
                    AcqRel,
                    Acquire,
                    guard,
                ) {
                    Ok((_, ptr)) => {
                        unbounded_ptr = ptr;
                    }
                    Err((_, actual)) => {
                        unbounded_ptr = actual;
                    }
                }
            }
            if let Some(unbounded) = unbounded_ptr.as_ref() {
                if !self.children.validate(metadata) {
                    continue;
                }
                let insert_result = unbounded.insert(key, val);
                match insert_result {
                    InsertResult::Success | InsertResult::Duplicate(..) => {
                        return Ok(insert_result);
                    }
                    InsertResult::Full(k, v) => {
                        match self.split_leaf(
                            unbounded_ptr,
                            &self.unbounded_child,
                            async_wait,
                            guard,
                        ) {
                            Ok(true) => {
                                key = k;
                                val = v;
                                continue;
                            }
                            Ok(false) => return Ok(InsertResult::Full(k, v)),
                            Err(()) => return Err((k, v)),
                        }
                    }
                    InsertResult::Frozen(k, v) => {
                        async_wait.try_wait(&self.lock);
                        return Err((k, v));
                    }
                };
            }
            return Ok(InsertResult::Full(key, val));
        }
    }

    /// Removes an entry associated with the given key.
    ///
    /// # Errors
    ///
    /// Returns an error if a retry is required.
    #[inline]
    pub(super) fn remove_if<Q, F: FnMut(&V) -> bool, W: TryWait>(
        &self,
        key: &Q,
        condition: &mut F,
        async_wait: &mut W,
        guard: &Guard,
    ) -> Result<RemoveResult, ()>
    where
        Q: Comparable<K> + ?Sized,
    {
        loop {
            let (child, metadata) = self.children.min_greater_equal(key);
            if let Some((_, child)) = child {
                let child_ptr = child.load(Acquire, guard);
                if let Some(child) = child_ptr.as_ref() {
                    if self.children.validate(metadata) {
                        // Data race resolution - see `LeafNode::search_entry`.
                        let result = child.remove_if(key, condition);
                        if result == RemoveResult::Frozen {
                            // Its entries may be being relocated.
                            async_wait.try_wait(&self.lock);
                            return Err(());
                        } else if result == RemoveResult::Retired {
                            return Ok(self.post_remove(guard));
                        }
                        return Ok(result);
                    }
                }
                // It is not a hot loop - see `LeafNode::search_entry`.
                continue;
            }
            let unbounded_ptr = self.unbounded_child.load(Acquire, guard);
            if let Some(unbounded) = unbounded_ptr.as_ref() {
                if !self.children.validate(metadata) {
                    // Data race resolution - see `LeafNode::search_entry`.
                    continue;
                }
                let result = unbounded.remove_if(key, condition);
                if result == RemoveResult::Frozen {
                    async_wait.try_wait(&self.lock);
                    return Err(());
                } else if result == RemoveResult::Retired {
                    return Ok(self.post_remove(guard));
                }
                return Ok(result);
            }
            return Ok(RemoveResult::Fail);
        }
    }

    /// Removes a range of entries.
    ///
    /// Returns the number of remaining children.
    #[inline]
    pub(super) fn remove_range<'g, Q, R: RangeBounds<Q>, W: TryWait>(
        &self,
        range: &R,
        start_unbounded: bool,
        valid_lower_max_leaf: Option<&'g Leaf<K, V>>,
        valid_upper_min_node: Option<&'g Node<K, V>>,
        async_wait: &mut W,
        guard: &'g Guard,
    ) -> Result<usize, ()>
    where
        Q: Comparable<K> + ?Sized,
    {
        debug_assert!(valid_lower_max_leaf.is_none() || start_unbounded);
        debug_assert!(valid_lower_max_leaf.is_none() || valid_upper_min_node.is_none());

        let Some(_lock) = Locker::try_lock(self) else {
            async_wait.try_wait(&self.lock);
            return Err(());
        };

        let mut current_state = RemoveRangeState::Below;
        let mut num_leaves = 1;
        let mut first_valid_leaf = None;

        let mut iter = Iter::new(&self.children);
        while let Some((key, leaf)) = iter.next() {
            current_state = current_state.next(key, range, start_unbounded);
            match current_state {
                RemoveRangeState::Below | RemoveRangeState::MaybeBelow => {
                    if let Some(leaf) = leaf.load(Acquire, guard).as_ref() {
                        leaf.remove_range(range);
                    }
                    num_leaves += 1;
                    if first_valid_leaf.is_none() {
                        first_valid_leaf.replace(leaf);
                    }
                }
                RemoveRangeState::FullyContained => {
                    if let Some(leaf) = leaf.swap((None, Tag::None), AcqRel).0 {
                        leaf.unlink(guard);
                    }
                    // There can be another thread inserting keys into the leaf, and this may render
                    // those operations completely ineffective.
                    iter.remove_unchecked();
                }
                RemoveRangeState::MaybeAbove => {
                    if let Some(leaf) = leaf.load(Acquire, guard).as_ref() {
                        leaf.remove_range(range);
                    }
                    num_leaves += 1;
                    if first_valid_leaf.is_none() {
                        first_valid_leaf.replace(leaf);
                    }
                    break;
                }
            }
        }

        if let Some(unbounded) = self.unbounded_child.load(Acquire, guard).as_ref() {
            unbounded.remove_range(range);
        }

        if let Some(valid_lower_max_leaf) = valid_lower_max_leaf {
            // Connect the specified leaf with the first valid leaf.
            if first_valid_leaf.is_none() {
                first_valid_leaf.replace(&self.unbounded_child);
            }
            let first_valid_leaf_ptr =
                first_valid_leaf.map_or(Ptr::null(), |l| l.load(Acquire, guard));
            valid_lower_max_leaf
                .next
                .store(first_valid_leaf_ptr.as_ptr().cast_mut(), Release);
            if let Some(first_valid_leaf) = first_valid_leaf_ptr.as_ref() {
                first_valid_leaf
                    .prev
                    .store(ptr::from_ref(valid_lower_max_leaf).cast_mut(), Release);
            }
        } else if let Some(valid_upper_min_node) = valid_upper_min_node {
            // Connect the unbounded child with the minimum valid leaf in the node.
            valid_upper_min_node.remove_range(
                range,
                true,
                self.unbounded_child.load(Acquire, guard).as_ref(),
                None,
                async_wait,
                guard,
            )?;
        }

        Ok(num_leaves)
    }

    /// Splits a full leaf.
    ///
    /// Returns `false` if the parent node needs to be split.
    ///
    /// # Errors
    ///
    /// Returns an error if locking failed or the full leaf node was changed.
    fn split_leaf<W: TryWait>(
        &self,
        full_leaf_ptr: Ptr<Leaf<K, V>>,
        full_leaf: &AtomicShared<Leaf<K, V>>,
        async_wait: &mut W,
        guard: &Guard,
    ) -> Result<bool, ()> {
        if self.is_retired() {
            // Let the parent node clean up this node.
            return Ok(false);
        }

        let Some(_locker) = Locker::try_lock(self) else {
            async_wait.try_wait(&self.lock);
            return Err(());
        };

        if self.unbounded_child.tag(Relaxed) != Tag::None
            || full_leaf_ptr != full_leaf.load(Relaxed, guard)
        {
            // The leaf node is being cleared, or the leaf node was already split.
            return Err(());
        }

        let target = full_leaf_ptr.as_ref().unwrap();
        let frozen = target.freeze();
        debug_assert!(frozen);

        let exit_guard = ExitGuard::new((), |()| {
            target.unfreeze();
        });

        let low_key_leaf = Shared::new(Leaf::new());
        let high_key_leaf = Leaf::new();
        let is_full = self.children.is_full();

        // Need to freeze new leaves before making them reachable.
        let frozen_low = low_key_leaf.freeze();
        let frozen_high = high_key_leaf.freeze();
        debug_assert!(frozen_low && frozen_high);

        let mut is_high_key_leaf_empty = true;
        if !target.distribute(|k, v, i, b, l| {
            if b < l && is_full {
                // E.g., `b == 2, l == 2`, then `i` can be as large as `1`: `high_key_leaf` is not
                // needed.
                return false;
            }
            // `v` is moved, not cloned; those new leaves do not own them until unfrozen.
            let v = unsafe { ptr::from_ref(v).read() };
            if i < b {
                low_key_leaf.insert_unchecked(k.clone(), v, i);
            } else {
                high_key_leaf.insert_unchecked(k.clone(), v, i - b);
                is_high_key_leaf_empty = false;
            }
            true
        }) {
            return Ok(false);
        }

        if is_high_key_leaf_empty {
            target.replace_link(
                |prev, next, _| {
                    low_key_leaf.prev.store(target.prev.load(Acquire), Relaxed);
                    low_key_leaf.next.store(target.next.load(Acquire), Relaxed);
                    if let Some(prev) = prev {
                        prev.next.store(low_key_leaf.as_ptr().cast_mut(), Release);
                    }
                    if let Some(next) = next {
                        next.prev.store(low_key_leaf.as_ptr().cast_mut(), Release);
                    }
                    // From here, `Iter` can reach the new leaf.
                },
                guard,
            );

            // Unfreeze the leaves; the leaf now takes ownership of the copied values.
            let unfrozen_low = low_key_leaf.unfreeze();
            debug_assert!(unfrozen_low);
            full_leaf.swap((Some(low_key_leaf), Tag::None), Release);
        } else {
            let low_key_max = low_key_leaf.max_key().unwrap().clone();
            let high_key_leaf = Shared::new(high_key_leaf);
            low_key_leaf
                .next
                .store(high_key_leaf.as_ptr().cast_mut(), Relaxed);
            high_key_leaf
                .prev
                .store(low_key_leaf.as_ptr().cast_mut(), Relaxed);

            target.replace_link(
                |prev, next, _| {
                    low_key_leaf.prev.store(target.prev.load(Acquire), Relaxed);
                    high_key_leaf.next.store(target.next.load(Acquire), Relaxed);
                    if let Some(prev) = prev {
                        prev.next.store(low_key_leaf.as_ptr().cast_mut(), Release);
                    }
                    if let Some(next) = next {
                        next.prev.store(high_key_leaf.as_ptr().cast_mut(), Release);
                    }
                    // From here, `Iter` can reach the new leaf.
                },
                guard,
            );

            // Take the max key value stored in the low key leaf as the leaf key.
            let result = self
                .children
                .insert(low_key_max, AtomicShared::from(low_key_leaf.clone()));
            debug_assert!(matches!(result, InsertResult::Success));

            // Unfreeze the leaves; those leaves now take ownership of the copied values.
            let unfrozen_low = low_key_leaf.unfreeze();
            let unfrozen_high = high_key_leaf.unfreeze();
            debug_assert!(unfrozen_low && unfrozen_high);
            full_leaf.swap((Some(high_key_leaf), Tag::None), Release);
        }

        // The removed leaf stays frozen: ownership of the copied values is transferred.
        exit_guard.forget();

        // If there was a clear operation in the meantime, new leaves will need to be cleaned up.
        if self.unbounded_child.tag(Acquire) != Tag::None {
            self.clear(guard);
        }

        Ok(true)
    }

    /// Tries to delete retired leaves after a successful removal of an entry.
    fn post_remove(&self, guard: &Guard) -> RemoveResult {
        let Some(lock) = Locker::try_lock(self) else {
            if self.is_retired() {
                return RemoveResult::Retired;
            }
            return RemoveResult::Success;
        };

        let mut prev_valid_leaf = None;
        let mut iter = Iter::new(&self.children);
        while let Some(entry) = iter.next() {
            let leaf_ptr = entry.1.load(Acquire, guard);
            let leaf = leaf_ptr.as_ref().unwrap();
            if leaf.is_retired() {
                leaf.unlink(guard);

                // As soon as the leaf is removed from the leaf node, the next leaf can store keys
                // that are smaller than those that were previously stored in the removed leaf node.
                //
                // Therefore, when unlinking a leaf, the current snapshot of metadata of neighboring
                // leaves is stored inside the leaf which will be used by iterators.
                let result = iter.remove_unchecked();
                debug_assert_ne!(result, RemoveResult::Fail);

                // The pointer is set to null after the metadata of `self.children` is updated
                // to enable readers to retry when they find it being null.
                entry.1.swap((None, Tag::None), Release);
            } else {
                prev_valid_leaf.replace(leaf);
            }
        }

        // The unbounded leaf becomes unreachable after all the other leaves are gone.
        let fully_empty = if prev_valid_leaf.is_some() {
            false
        } else {
            let unbounded_ptr = self.unbounded_child.load(Acquire, guard);
            if let Some(unbounded) = unbounded_ptr.as_ref() {
                if unbounded.is_retired() {
                    unbounded.unlink(guard);

                    // `Tag::First` prevents `insert` from allocating a new leaf.
                    self.unbounded_child.swap((None, Tag::First), Release);
                    true
                } else {
                    false
                }
            } else {
                true
            }
        };

        if fully_empty {
            lock.unlock_retire();
            RemoveResult::Retired
        } else {
            RemoveResult::Success
        }
    }
}

impl<'n, K, V> Locker<'n, K, V> {
    /// Acquires exclusive lock on the [`LeafNode`].
    #[inline]
    pub(super) fn try_lock(leaf_node: &'n LeafNode<K, V>) -> Option<Locker<'n, K, V>> {
        if leaf_node.lock.try_lock() {
            Some(Locker { leaf_node })
        } else {
            None
        }
    }

    /// Retires the leaf node by poisoning the lock.
    #[inline]
    pub(super) fn unlock_retire(self) {
        self.leaf_node.lock.poison_lock();
        forget(self);
    }
}

impl<K, V> Drop for Locker<'_, K, V> {
    #[inline]
    fn drop(&mut self) {
        self.leaf_node.lock.release_lock();
    }
}

impl RemoveRangeState {
    /// Returns the next state.
    pub(super) fn next<K, Q, R: RangeBounds<Q>>(
        self,
        key: &K,
        range: &R,
        start_unbounded: bool,
    ) -> Self
    where
        Q: Comparable<K> + ?Sized,
    {
        if range_contains(range, key) {
            match self {
                RemoveRangeState::Below => {
                    if start_unbounded {
                        RemoveRangeState::FullyContained
                    } else {
                        RemoveRangeState::MaybeBelow
                    }
                }
                RemoveRangeState::MaybeBelow | RemoveRangeState::FullyContained => {
                    RemoveRangeState::FullyContained
                }
                RemoveRangeState::MaybeAbove => unreachable!(),
            }
        } else {
            match self {
                RemoveRangeState::Below => match range.start_bound() {
                    Bound::Included(k) => match k.compare(key) {
                        Less | Equal => RemoveRangeState::MaybeAbove,
                        Greater => RemoveRangeState::Below,
                    },
                    Bound::Excluded(k) => match k.compare(key) {
                        Less => RemoveRangeState::MaybeAbove,
                        Greater | Equal => RemoveRangeState::Below,
                    },
                    Bound::Unbounded => RemoveRangeState::MaybeAbove,
                },
                RemoveRangeState::MaybeBelow | RemoveRangeState::FullyContained => {
                    RemoveRangeState::MaybeAbove
                }
                RemoveRangeState::MaybeAbove => unreachable!(),
            }
        }
    }
}

#[cfg(not(feature = "loom"))]
#[cfg(test)]
mod test {
    use super::*;
    use std::sync::atomic::AtomicBool;
    use tokio::sync::Barrier;

    #[test]
    fn basic() {
        let guard = Guard::new();
        let leaf_node: LeafNode<String, String> = LeafNode::new();
        assert!(matches!(
            leaf_node.insert(
                "MY GOODNESS!".to_owned(),
                "OH MY GOD!!".to_owned(),
                &mut (),
                &guard
            ),
            Ok(InsertResult::Success)
        ));
        assert!(matches!(
            leaf_node.insert(
                "GOOD DAY".to_owned(),
                "OH MY GOD!!".to_owned(),
                &mut (),
                &guard
            ),
            Ok(InsertResult::Success)
        ));
        assert_eq!(
            leaf_node.search_entry("MY GOODNESS!", &guard).unwrap().1,
            "OH MY GOD!!"
        );
        assert_eq!(
            leaf_node.search_entry("GOOD DAY", &guard).unwrap().1,
            "OH MY GOD!!"
        );
        assert!(matches!(
            leaf_node.remove_if::<_, _, _>("GOOD DAY", &mut |v| v == "OH MY", &mut (), &guard),
            Ok(RemoveResult::Fail)
        ));
        assert!(matches!(
            leaf_node.remove_if::<_, _, _>(
                "GOOD DAY",
                &mut |v| v == "OH MY GOD!!",
                &mut (),
                &guard
            ),
            Ok(RemoveResult::Success)
        ));
        assert!(matches!(
            leaf_node.remove_if::<_, _, _>("GOOD", &mut |v| v == "OH MY", &mut (), &guard),
            Ok(RemoveResult::Fail)
        ));
        assert!(matches!(
            leaf_node.remove_if::<_, _, _>("MY GOODNESS!", &mut |_| true, &mut (), &guard),
            Ok(RemoveResult::Retired)
        ));
        assert!(matches!(
            leaf_node.insert("HI".to_owned(), "HO".to_owned(), &mut (), &guard),
            Ok(InsertResult::Full(..))
        ));
    }

    #[test]
    fn bulk() {
        let guard = Guard::new();
        let leaf_node: LeafNode<usize, usize> = LeafNode::new();
        for k in 0..1024 {
            let mut result = leaf_node.insert(k, k, &mut (), &guard);
            if result.is_err() {
                result = leaf_node.insert(k, k, &mut (), &guard);
            }
            match result.unwrap() {
                InsertResult::Success => {
                    assert_eq!(leaf_node.search_entry(&k, &guard), Some((&k, &k)));
                }
                InsertResult::Duplicate(..) | InsertResult::Frozen(..) => unreachable!(),
                InsertResult::Full(_, _) => {
                    for r in 0..(k - 1) {
                        assert_eq!(leaf_node.search_entry(&r, &guard), Some((&r, &r)));
                        assert!(
                            leaf_node
                                .remove_if::<_, _, _>(&r, &mut |_| true, &mut (), &guard)
                                .is_ok()
                        );
                        assert_eq!(leaf_node.search_entry(&r, &guard), None);
                    }
                    assert_eq!(
                        leaf_node.search_entry(&(k - 1), &guard),
                        Some((&(k - 1), &(k - 1)))
                    );
                    assert_eq!(
                        leaf_node.remove_if::<_, _, _>(&(k - 1), &mut |_| true, &mut (), &guard),
                        Ok(RemoveResult::Retired)
                    );
                    assert_eq!(leaf_node.search_entry(&(k - 1), &guard), None);
                    break;
                }
            }
        }
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 16)]
    async fn parallel() {
        let num_tasks = 8;
        let workload_size = 64;
        let barrier = Shared::new(Barrier::new(num_tasks));
        for _ in 0..16 {
            let leaf_node = Shared::new(LeafNode::new());
            assert!(
                leaf_node
                    .insert(usize::MAX, usize::MAX, &mut (), &Guard::new())
                    .is_ok()
            );
            let mut task_handles = Vec::with_capacity(num_tasks);
            for task_id in 0..num_tasks {
                let barrier_clone = barrier.clone();
                let leaf_node_clone = leaf_node.clone();
                task_handles.push(tokio::task::spawn(async move {
                    barrier_clone.wait().await;
                    let guard = Guard::new();
                    let mut max_key = None;
                    let range = (task_id * workload_size)..((task_id + 1) * workload_size);
                    for id in range.clone() {
                        loop {
                            if let Ok(r) = leaf_node_clone.insert(id, id, &mut (), &guard) {
                                match r {
                                    InsertResult::Success => {
                                        match leaf_node_clone.insert(id, id, &mut (), &guard) {
                                            Ok(InsertResult::Duplicate(..)) | Err(_) => (),
                                            _ => unreachable!(),
                                        }
                                        break;
                                    }
                                    InsertResult::Full(..) => {
                                        max_key.replace(id);
                                        break;
                                    }
                                    InsertResult::Duplicate(..) | InsertResult::Frozen(..) => {
                                        unreachable!()
                                    }
                                }
                            }
                        }
                        if max_key.is_some() {
                            break;
                        }
                    }
                    for id in range.clone() {
                        if max_key == Some(id) {
                            break;
                        }
                        assert_eq!(leaf_node_clone.search_entry(&id, &guard), Some((&id, &id)));
                    }
                    for id in range {
                        if max_key == Some(id) {
                            break;
                        }
                        loop {
                            if let Ok(r) = leaf_node_clone.remove_if::<_, _, _>(
                                &id,
                                &mut |_| true,
                                &mut (),
                                &guard,
                            ) {
                                match r {
                                    RemoveResult::Success | RemoveResult::Fail => break,
                                    RemoveResult::Frozen | RemoveResult::Retired => unreachable!(),
                                }
                            }
                        }
                        assert!(
                            leaf_node_clone.search_entry(&id, &guard).is_none(),
                            "{}",
                            id
                        );
                        if let Ok(RemoveResult::Success) = leaf_node_clone.remove_if::<_, _, _>(
                            &id,
                            &mut |_| true,
                            &mut (),
                            &guard,
                        ) {
                            unreachable!()
                        }
                    }
                }));
            }

            for r in futures::future::join_all(task_handles).await {
                assert!(r.is_ok());
            }
            assert!(
                leaf_node
                    .remove_if::<_, _, _>(&usize::MAX, &mut |_| true, &mut (), &Guard::new())
                    .is_ok()
            );
        }
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 16)]
    async fn durability() {
        let num_tasks = 8_usize;
        let workload_size = 64_usize;
        for _ in 0..16 {
            for k in 0..=workload_size {
                let barrier = Shared::new(Barrier::new(num_tasks));
                let leaf_node: Shared<LeafNode<usize, usize>> = Shared::new(LeafNode::new());
                let inserted: Shared<AtomicBool> = Shared::new(AtomicBool::new(false));
                let mut task_handles = Vec::with_capacity(num_tasks);
                for _ in 0..num_tasks {
                    let barrier_clone = barrier.clone();
                    let leaf_node_clone = leaf_node.clone();
                    let inserted_clone = inserted.clone();
                    task_handles.push(tokio::spawn(async move {
                        {
                            barrier_clone.wait().await;
                            let guard = Guard::new();
                            if let Ok(InsertResult::Success) =
                                leaf_node_clone.insert(k, k, &mut (), &guard)
                            {
                                assert!(!inserted_clone.swap(true, Relaxed));
                            }
                        }
                        {
                            barrier_clone.wait().await;
                            let guard = Guard::new();
                            for i in 0..workload_size {
                                if i != k {
                                    let result = leaf_node_clone.insert(i, i, &mut (), &guard);
                                    drop(result);
                                }
                                assert_eq!(
                                    leaf_node_clone.search_entry(&k, &guard).unwrap(),
                                    (&k, &k)
                                );
                            }
                            for i in 0..workload_size {
                                let max_iter =
                                    leaf_node_clone.approximate::<_, true>(&k, &guard).unwrap();
                                assert!(*max_iter.get().unwrap().0 <= k);
                                let mut min_iter = leaf_node_clone.min(&guard).unwrap();
                                if let Some((k_ref, v_ref)) = min_iter.next() {
                                    assert_eq!(*k_ref, *v_ref);
                                    assert!(*k_ref <= k);
                                } else {
                                    let (k_ref, v_ref) =
                                        min_iter.jump(&guard).unwrap().get().unwrap();
                                    assert_eq!(*k_ref, *v_ref);
                                    assert!(*k_ref <= k);
                                }
                                let _result = leaf_node_clone.remove_if::<_, _, _>(
                                    &i,
                                    &mut |v| *v != k,
                                    &mut (),
                                    &guard,
                                );
                                assert_eq!(
                                    leaf_node_clone.search_entry(&k, &guard).unwrap(),
                                    (&k, &k)
                                );
                            }
                        }
                    }));
                }
                for r in futures::future::join_all(task_handles).await {
                    assert!(r.is_ok());
                }
                assert!((*inserted).load(Relaxed));
            }
        }
    }
}
