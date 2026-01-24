use std::cell::UnsafeCell;
use std::cmp::Ordering;
use std::fmt::{self, Debug};
use std::mem::{MaybeUninit, needs_drop};
use std::ops::Bound::{Excluded, Included, Unbounded};
use std::ops::RangeBounds;
use std::ptr;
use std::sync::atomic::Ordering::{AcqRel, Acquire, Relaxed, Release};
#[cfg(not(feature = "loom"))]
use std::sync::atomic::{AtomicPtr, AtomicUsize};

use saa::Lock;
use sdd::Guard;

use crate::Comparable;
#[cfg(feature = "loom")]
use loom::sync::atomic::{AtomicPtr, AtomicUsize};

/// [`Leaf`] is an ordered array of key-value pairs.
///
/// A constructed key-value pair entry is never dropped until the entire [`Leaf`] instance is
/// dropped.
pub struct Leaf<K, V> {
    /// The metadata containing information about the [`Leaf`] and individual entries.
    ///
    /// The state of each entry is as follows.
    /// * `0`: `uninit`.
    /// * `1-ARRAY_SIZE`: `rank`.
    /// * `ARRAY_SIZE + 1`: `removed`.
    ///
    /// The entry state transitions as follows.
    /// * `uninit -> removed -> rank -> removed`.
    metadata: AtomicUsize,
    /// Entry array.
    entry_array: UnsafeCell<EntryArray<K, V>>,
    /// Lock to protect the linked list.
    lock: Lock,
    /// Pointer to the previous [`Leaf`].
    pub(super) prev: AtomicPtr<Leaf<K, V>>,
    /// Pointer to the next [`Leaf`].
    pub(super) next: AtomicPtr<Leaf<K, V>>,
}

/// The number of entries and number of state bits per entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Dimension {
    pub num_entries: usize,
    pub num_bits_per_entry: usize,
}

/// Insertion result.
pub enum InsertResult<K, V> {
    /// Insertion succeeded.
    Success,
    /// Duplicate key found.
    Duplicate(K, V),
    /// No vacant slot for the key.
    Full(K, V),
    /// The [`Leaf`] is frozen.
    ///
    /// This is not a terminal state as a frozen [`Leaf`] can be unfrozen.
    Frozen(K, V),
}

/// Remove result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RemoveResult {
    /// Remove succeeded.
    Success,
    /// Remove succeeded and cleanup required.
    Retired,
    /// Remove failed.
    Fail,
    /// The [`Leaf`] is frozen.
    Frozen,
}

/// Each constructed entry in an `EntryArray` is never dropped until the [`Leaf`] is dropped.
pub type EntryArray<K, V> = (
    [MaybeUninit<K>; DIMENSION.num_entries],
    [MaybeUninit<V>; DIMENSION.num_entries],
);

/// Leaf entry iterator.
pub struct Iter<'l, K, V> {
    leaf: &'l Leaf<K, V>,
    metadata: usize,
    index: [u8; DIMENSION.num_entries],
    rank: u8,
}

/// Leaf entry iterator, reversed.
pub struct RevIter<'l, K, V> {
    leaf: &'l Leaf<K, V>,
    metadata: usize,
    index: [u8; DIMENSION.num_entries],
    rev_rank: u8,
}

/// Emulates `RangeBounds::contains`.
#[inline]
pub(crate) fn range_contains<K, Q, R: RangeBounds<Q>>(range: &R, key: &K) -> bool
where
    Q: Comparable<K> + ?Sized,
{
    (match range.start_bound() {
        Included(start) => start.compare(key).is_le(),
        Excluded(start) => start.compare(key).is_lt(),
        Unbounded => true,
    }) && (match range.end_bound() {
        Included(end) => end.compare(key).is_ge(),
        Excluded(end) => end.compare(key).is_gt(),
        Unbounded => true,
    })
}

impl<K, V> Leaf<K, V> {
    /// Creates a new [`Leaf`].
    #[cfg(not(feature = "loom"))]
    #[inline]
    pub(super) const fn new() -> Leaf<K, V> {
        #[allow(clippy::uninit_assumed_init)]
        Leaf {
            metadata: AtomicUsize::new(0),
            entry_array: UnsafeCell::new(unsafe { MaybeUninit::uninit().assume_init() }),
            lock: Lock::new(),
            prev: AtomicPtr::new(ptr::null_mut()),
            next: AtomicPtr::new(ptr::null_mut()),
        }
    }

    #[cfg(feature = "loom")]
    #[inline]
    pub(super) fn new() -> Leaf<K, V> {
        #[allow(clippy::uninit_assumed_init)]
        Leaf {
            metadata: AtomicUsize::new(0),
            entry_array: UnsafeCell::new(unsafe { MaybeUninit::uninit().assume_init() }),
            lock: Lock::new(),
            prev: AtomicPtr::new(ptr::null_mut()),
            next: AtomicPtr::new(ptr::null_mut()),
        }
    }

    /// Returns `true` if the [`Leaf`] has no reachable entry.
    #[inline]
    pub(super) fn is_empty(&self) -> bool {
        Self::len(self.metadata.load(Relaxed)) == 0
    }

    /// Checks if the leaf is full or retired.
    #[inline]
    pub(super) fn is_full(&self) -> bool {
        let metadata = self.metadata.load(Relaxed);
        let rank = DIMENSION.rank(metadata, DIMENSION.num_entries - 1);
        rank != Dimension::uninit_rank() || Dimension::retired(metadata)
    }

    /// Returns `true` if the [`Leaf`] has retired.
    #[inline]
    pub(super) fn is_retired(&self) -> bool {
        Dimension::retired(self.metadata.load(Acquire))
    }

    /// Replaces itself in the linked list with others as defined in the specified closure.
    #[inline]
    pub(super) fn replace_link<F: FnOnce(Option<&Self>, Option<&Self>, &Guard)>(
        &self,
        f: F,
        guard: &Guard,
    ) {
        let mut prev = self.prev.load(Acquire);
        loop {
            if let Some(prev_link) = unsafe { prev.as_ref() } {
                prev_link.lock.lock_sync();
            }
            self.lock.lock_sync();
            let prev_check = self.prev.load(Acquire);
            if prev_check == prev {
                break;
            }
            if let Some(prev_link) = unsafe { prev.as_ref() } {
                prev_link.lock.release_lock();
            }
            self.lock.release_lock();
            prev = prev_check;
        }
        let prev = unsafe { prev.as_ref() };
        let next = unsafe { self.next.load(Acquire).as_ref() };
        if let Some(next_link) = next {
            next_link.lock.lock_sync();
        }

        f(prev, next, guard);

        self.metadata.fetch_or(Dimension::UNLINKED, Release);
        if let Some(prev_link) = prev {
            let released = prev_link.lock.release_lock();
            debug_assert!(released);
        }
        let released = self.lock.release_lock();
        debug_assert!(released);
        if let Some(next_link) = next {
            let released = next_link.lock.release_lock();
            debug_assert!(released);
        }
    }

    /// Deletes itself from the linked list.
    #[inline]
    pub(super) fn unlink(&self, guard: &Guard) {
        self.replace_link(
            |prev, next, _| {
                if let Some(prev_link) = prev {
                    prev_link.next.store(self.next.load(Acquire), Release);
                }
                if let Some(next_link) = next {
                    next_link.prev.store(self.prev.load(Acquire), Release);
                }
            },
            guard,
        );
    }

    /// Returns the number of reachable entries.
    #[inline]
    pub(super) const fn len(metadata: usize) -> usize {
        let mut mutable_metadata = metadata;
        let mut count = 0;
        let mut i = 0;
        while i != DIMENSION.num_entries {
            if mutable_metadata == 0 {
                break;
            }
            let rank = mutable_metadata % (1_usize << DIMENSION.num_bits_per_entry);
            if rank != Dimension::uninit_rank() && rank != DIMENSION.removed_rank() {
                count += 1;
            }
            mutable_metadata >>= DIMENSION.num_bits_per_entry;
            i += 1;
        }
        count
    }

    /// Returns a reference to the max key.
    #[inline]
    pub(super) fn max_key(&self) -> Option<&K> {
        let mut mutable_metadata = self.metadata.load(Acquire);
        let mut max_rank = 0;
        let mut max_index = DIMENSION.num_entries;
        for i in 0..DIMENSION.num_entries {
            if mutable_metadata == 0 {
                break;
            }
            let rank = mutable_metadata % (1_usize << DIMENSION.num_bits_per_entry);
            if rank > max_rank && rank != DIMENSION.removed_rank() {
                max_rank = rank;
                max_index = i;
            }
            mutable_metadata >>= DIMENSION.num_bits_per_entry;
        }
        if max_index != DIMENSION.num_entries {
            return Some(self.key_at(max_index));
        }
        None
    }

    /// Inserts a key value pair at the specified position without checking the metadata.
    ///
    /// `rank` is calculated as `index + 1`.
    #[inline]
    pub(super) fn insert_unchecked(&self, key: K, val: V, index: usize) {
        debug_assert!(index < DIMENSION.num_entries);
        let metadata = self.metadata.load(Relaxed);
        let new_metadata = DIMENSION.augment(metadata, index, index + 1);
        self.write(index, key, val);
        self.metadata.store(new_metadata, Release);
    }

    /// Removes the entry at the specified position without checking the metadata.
    #[inline]
    pub(super) fn remove_unchecked(&self, mut metadata: usize, index: usize) -> RemoveResult {
        loop {
            let mut empty = true;
            let mut mutable_metadata = metadata;
            for j in 0..DIMENSION.num_entries {
                if mutable_metadata == 0 {
                    break;
                }
                if index != j {
                    let rank = mutable_metadata % (1_usize << DIMENSION.num_bits_per_entry);
                    if rank != Dimension::uninit_rank() && rank != DIMENSION.removed_rank() {
                        empty = false;
                        break;
                    }
                }
                mutable_metadata >>= DIMENSION.num_bits_per_entry;
            }

            let mut new_metadata = metadata | DIMENSION.rank_mask(index);
            if empty {
                new_metadata = Dimension::retire(new_metadata);
            }
            match self
                .metadata
                .compare_exchange(metadata, new_metadata, AcqRel, Acquire)
            {
                Ok(_) => {
                    if empty {
                        return RemoveResult::Retired;
                    }
                    return RemoveResult::Success;
                }
                Err(actual) => {
                    if DIMENSION.rank(actual, index) == DIMENSION.removed_rank() {
                        return RemoveResult::Fail;
                    }
                    if Dimension::frozen(actual) {
                        return RemoveResult::Frozen;
                    }
                    metadata = actual;
                }
            }
        }
    }

    /// Compares the given metadata value with the current one.
    #[inline]
    pub(super) fn validate(&self, metadata: usize) -> bool {
        // `Relaxed` is sufficient as long as the caller has load-acquired its contents.
        self.metadata.load(Relaxed) == metadata
    }

    /// Freezes the [`Leaf`] temporarily.
    ///
    /// A frozen [`Leaf`] cannot store more entries, and on-going insertion is canceled.
    #[inline]
    pub(super) fn freeze(&self) -> bool {
        self.metadata
            .fetch_update(AcqRel, Acquire, |p| {
                if Dimension::frozen(p) {
                    None
                } else {
                    Some(Dimension::freeze(p))
                }
            })
            .is_ok()
    }

    /// Unfreezes the [`Leaf`].
    #[inline]
    pub(super) fn unfreeze(&self) -> bool {
        self.metadata
            .fetch_update(Release, Relaxed, |p| {
                if Dimension::frozen(p) {
                    Some(Dimension::unfreeze(p))
                } else {
                    None
                }
            })
            .is_ok()
    }

    /// Returns the recommended number of entries that the left-side node should store when a
    /// [`Leaf`] is split.
    ///
    /// Returns a number in `[1, len(leaf))` that represents the recommended number of entries in
    /// the left-side node. The number is calculated as follows for each adjacent slot:
    /// - Initial `score = len(leaf)`.
    /// - Rank increased: `score -= 1`.
    /// - Rank decreased: `score += 1`.
    /// - Clamp `score` in `[len(leaf) / 2 + 1, len(leaf) / 2 + len(leaf) - 1)`.
    /// - Take `score - len(leaf) / 2`.
    ///
    /// For instance, when the length of a [`Leaf`] is 7,
    /// - Returns 6 for `rank = [1, 2, 3, 4, 5, 6, 7]`.
    /// - Returns 1 for `rank = [7, 6, 5, 4, 3, 2, 1]`.
    #[inline]
    pub(super) fn optimal_boundary(mut mutable_metadata: usize) -> usize {
        let mut boundary: usize = DIMENSION.num_entries;
        let mut prev_rank = 0;
        for _ in 0..DIMENSION.num_entries {
            let rank = mutable_metadata % (1_usize << DIMENSION.num_bits_per_entry);
            if rank != 0 && rank != DIMENSION.removed_rank() {
                if prev_rank >= rank {
                    boundary -= 1;
                } else if prev_rank != 0 {
                    boundary += 1;
                }
                prev_rank = rank;
            }
            mutable_metadata >>= DIMENSION.num_bits_per_entry;
        }
        boundary.clamp(
            DIMENSION.num_entries / 2 + 1,
            DIMENSION.num_entries + DIMENSION.num_entries / 2 - 1,
        ) - DIMENSION.num_entries / 2
    }

    /// Returns a reference to the key at the given index.
    #[inline]
    const fn key_at(&self, index: usize) -> &K {
        unsafe { &*(*self.entry_array.get()).0[index].as_ptr() }
    }

    /// Returns a reference to the key at the given index.
    #[inline]
    const fn value_at(&self, index: usize) -> &V {
        unsafe { &*(*self.entry_array.get()).1[index].as_ptr() }
    }

    /// Writes the key and value at the given index.
    #[inline]
    const fn write(&self, index: usize, key: K, val: V) {
        unsafe {
            (*self.entry_array.get()).0[index].as_mut_ptr().write(key);
            (*self.entry_array.get()).1[index].as_mut_ptr().write(val);
        }
    }

    /// Rolls back the insertion at the given index.
    fn rollback(&self, index: usize) -> (K, V) {
        let (k, v) = unsafe {
            (
                (*self.entry_array.get()).0[index].as_ptr().read(),
                (*self.entry_array.get()).1[index].as_ptr().read(),
            )
        };
        self.metadata
            .fetch_and(!DIMENSION.rank_mask(index), Release);
        (k, v)
    }

    /// Builds a rank to index map from metadata.
    #[allow(clippy::cast_possible_truncation)]
    #[inline]
    const fn build_index(metadata: usize) -> [u8; DIMENSION.num_entries] {
        let mut index = [0; DIMENSION.num_entries];
        let mut i = 0;
        while i != DIMENSION.num_entries {
            let rank = DIMENSION.rank(metadata, i);
            i += 1;
            if rank != Dimension::uninit_rank() && rank != DIMENSION.removed_rank() {
                index[rank - 1] = i as u8;
            }
        }
        index
    }
}

impl<K, V> Leaf<K, V>
where
    K: 'static + Ord,
    V: 'static,
{
    /// Inserts a key value pair.
    #[inline]
    pub(super) fn insert(&self, key: K, val: V) -> InsertResult<K, V> {
        let mut metadata = self.metadata.load(Acquire);
        'after_read_metadata: loop {
            if Dimension::retired(metadata) {
                return InsertResult::Full(key, val);
            } else if Dimension::frozen(metadata) {
                return InsertResult::Frozen(key, val);
            }

            let mut mutable_metadata = metadata;
            for i in 0..DIMENSION.num_entries {
                let rank = mutable_metadata % (1_usize << DIMENSION.num_bits_per_entry);
                if rank == Dimension::uninit_rank() {
                    let interim_metadata = DIMENSION.augment(metadata, i, DIMENSION.removed_rank());

                    // Reserve the slot.
                    //
                    // It doesn't have to be a release-store.
                    if let Err(actual) =
                        self.metadata
                            .compare_exchange(metadata, interim_metadata, Acquire, Acquire)
                    {
                        metadata = actual;
                        continue 'after_read_metadata;
                    }

                    self.write(i, key, val);
                    return self.post_insert(i, interim_metadata);
                }
                mutable_metadata >>= DIMENSION.num_bits_per_entry;
            }

            if self.search_slot(&key, metadata).is_some() {
                return InsertResult::Duplicate(key, val);
            }
            return InsertResult::Full(key, val);
        }
    }

    /// Removes the key if the condition is met.
    #[inline]
    pub(super) fn remove_if<Q, F: FnMut(&V) -> bool>(
        &self,
        key: &Q,
        condition: &mut F,
    ) -> RemoveResult
    where
        Q: Comparable<K> + ?Sized,
    {
        let metadata = self.metadata.load(Acquire);
        if Dimension::frozen(metadata) {
            return RemoveResult::Frozen;
        }
        let mut min_max_rank = DIMENSION.removed_rank();
        let mut max_min_rank = 0;
        let mut mutable_metadata = metadata;
        for i in 0..DIMENSION.num_entries {
            if mutable_metadata == 0 {
                break;
            }
            let rank = mutable_metadata % (1_usize << DIMENSION.num_bits_per_entry);
            if rank < min_max_rank && rank > max_min_rank {
                match self.compare(i, key) {
                    Ordering::Less => {
                        if max_min_rank < rank {
                            max_min_rank = rank;
                        }
                    }
                    Ordering::Greater => {
                        if min_max_rank > rank {
                            min_max_rank = rank;
                        }
                    }
                    Ordering::Equal => {
                        // Found the key.
                        if !condition(self.value_at(i)) {
                            // The given condition is not met.
                            return RemoveResult::Fail;
                        }
                        return self.remove_unchecked(metadata, i);
                    }
                }
            }
            mutable_metadata >>= DIMENSION.num_bits_per_entry;
        }

        RemoveResult::Fail
    }

    /// Removes a range of entries.
    ///
    /// Returns the number of remaining entries.
    #[inline]
    pub(super) fn remove_range<Q, R: RangeBounds<Q>>(&self, range: &R)
    where
        Q: Comparable<K> + ?Sized,
    {
        let mut mutable_metadata = self.metadata.load(Acquire);
        for i in 0..DIMENSION.num_entries {
            if mutable_metadata == 0 {
                break;
            }
            let rank = mutable_metadata % (1_usize << DIMENSION.num_bits_per_entry);
            if rank != Dimension::uninit_rank() && rank != DIMENSION.removed_rank() {
                let k = self.key_at(i);
                if range_contains(range, k) {
                    self.remove_if(k, &mut |_| true);
                }
            }
            mutable_metadata >>= DIMENSION.num_bits_per_entry;
        }
    }

    /// Returns an entry containing the specified key.
    #[inline]
    pub(super) fn search_entry<Q>(&self, key: &Q) -> Option<(&K, &V)>
    where
        Q: Comparable<K> + ?Sized,
    {
        let metadata = self.metadata.load(Acquire);
        self.search_slot(key, metadata)
            .map(|i| (self.key_at(i), self.value_at(i)))
    }

    /// Returns the value associated with the specified key.
    #[inline]
    pub(super) fn search_value<Q>(&self, key: &Q) -> Option<&V>
    where
        Q: Comparable<K> + ?Sized,
    {
        let metadata = self.metadata.load(Acquire);
        self.search_slot(key, metadata).map(|i| self.value_at(i))
    }

    /// Returns the minimum entry among those that are not `Ordering::Less` than the given key.
    ///
    /// It additionally returns the current version of its metadata so the caller can validate the
    /// correctness of the result.
    #[inline]
    pub(super) fn min_greater_equal<Q>(&self, key: &Q) -> (Option<(&K, &V)>, usize)
    where
        Q: Comparable<K> + ?Sized,
    {
        let metadata = self.metadata.load(Acquire);
        let mut min_max_rank = DIMENSION.removed_rank();
        let mut max_min_rank = 0;
        let mut min_max_index = DIMENSION.num_entries;
        let mut mutable_metadata = metadata;
        for i in 0..DIMENSION.num_entries {
            if mutable_metadata == 0 {
                break;
            }
            let rank = mutable_metadata % (1_usize << DIMENSION.num_bits_per_entry);
            if rank < min_max_rank && rank > max_min_rank {
                let k = self.key_at(i);
                match key.compare(k) {
                    Ordering::Greater => {
                        if max_min_rank < rank {
                            max_min_rank = rank;
                        }
                    }
                    Ordering::Less => {
                        if min_max_rank > rank {
                            min_max_rank = rank;
                            min_max_index = i;
                        }
                    }
                    Ordering::Equal => {
                        return (Some((k, self.value_at(i))), metadata);
                    }
                }
            }
            mutable_metadata >>= DIMENSION.num_bits_per_entry;
        }
        if min_max_index != DIMENSION.num_entries {
            return (
                Some((self.key_at(min_max_index), self.value_at(min_max_index))),
                metadata,
            );
        }
        (None, metadata)
    }

    /// Distributes entries to given leaves.
    ///
    /// `dist` is a function to distribute entries to other containers where the first argument is
    /// the key, the second argument is the value, the third argument is the index, the fourth
    /// argument is the boundary, and the fifth argument is the length. Stops distribution if the
    /// function returns `false`, and this method returns `false`.
    #[inline]
    pub(super) fn distribute<F: FnMut(&K, &V, usize, usize, usize) -> bool>(
        &self,
        mut dist: F,
    ) -> bool {
        let iter = Iter::new(self);
        let len = Self::len(iter.metadata);
        let boundary = Self::optimal_boundary(iter.metadata);
        for (i, (k, v)) in iter.enumerate() {
            if !dist(k, v, i, boundary, len) {
                return false;
            }
        }
        true
    }

    /// Post-processing after reserving a free slot.
    fn post_insert(&self, free_slot_index: usize, mut prev_metadata: usize) -> InsertResult<K, V> {
        let key = self.key_at(free_slot_index);
        loop {
            let mut min_max_rank = DIMENSION.removed_rank();
            let mut max_min_rank = 0;
            let mut new_metadata = prev_metadata;
            let mut mutable_metadata = prev_metadata;
            for i in 0..DIMENSION.num_entries {
                if mutable_metadata == 0 {
                    break;
                }
                let rank = mutable_metadata % (1_usize << DIMENSION.num_bits_per_entry);
                if rank < min_max_rank && rank > max_min_rank {
                    match self.compare(i, key) {
                        Ordering::Less => {
                            if max_min_rank < rank {
                                max_min_rank = rank;
                            }
                        }
                        Ordering::Greater => {
                            if min_max_rank > rank {
                                min_max_rank = rank;
                            }
                            new_metadata = DIMENSION.augment(new_metadata, i, rank + 1);
                        }
                        Ordering::Equal => {
                            // Duplicate key.
                            let (k, v) = self.rollback(free_slot_index);
                            return InsertResult::Duplicate(k, v);
                        }
                    }
                } else if rank != DIMENSION.removed_rank() && rank > min_max_rank {
                    new_metadata = DIMENSION.augment(new_metadata, i, rank + 1);
                }
                mutable_metadata >>= DIMENSION.num_bits_per_entry;
            }

            // Make the newly inserted value reachable.
            let final_metadata = DIMENSION.augment(new_metadata, free_slot_index, max_min_rank + 1);
            if let Err(actual) =
                self.metadata
                    .compare_exchange(prev_metadata, final_metadata, AcqRel, Acquire)
            {
                let frozen = Dimension::frozen(actual);
                let retired = Dimension::retired(actual);
                if frozen || retired {
                    let (k, v) = self.rollback(free_slot_index);
                    if frozen {
                        return InsertResult::Frozen(k, v);
                    }
                    return InsertResult::Full(k, v);
                }
                prev_metadata = actual;
                continue;
            }

            return InsertResult::Success;
        }
    }

    /// Searches for a slot in which the key is stored.
    #[inline]
    fn search_slot<Q>(&self, key: &Q, mut mutable_metadata: usize) -> Option<usize>
    where
        Q: Comparable<K> + ?Sized,
    {
        let mut min_max_rank = DIMENSION.removed_rank();
        let mut max_min_rank = 0;
        for i in 0..DIMENSION.num_entries {
            if mutable_metadata == 0 {
                break;
            }
            let rank = mutable_metadata % (1_usize << DIMENSION.num_bits_per_entry);
            if rank < min_max_rank && rank > max_min_rank {
                match self.compare(i, key) {
                    Ordering::Less => {
                        if max_min_rank < rank {
                            max_min_rank = rank;
                        }
                    }
                    Ordering::Greater => {
                        if min_max_rank > rank {
                            min_max_rank = rank;
                        }
                    }
                    Ordering::Equal => {
                        return Some(i);
                    }
                }
            }
            mutable_metadata >>= DIMENSION.num_bits_per_entry;
        }
        None
    }

    #[inline]
    fn compare<Q>(&self, index: usize, key: &Q) -> Ordering
    where
        Q: Comparable<K> + ?Sized,
    {
        key.compare(self.key_at(index)).reverse()
    }
}

impl<K, V> Drop for Leaf<K, V> {
    #[inline]
    fn drop(&mut self) {
        if needs_drop::<K>() || needs_drop::<V>() {
            let mut mutable_metadata = self.metadata.load(Acquire);
            let is_frozen = Dimension::frozen(mutable_metadata);
            for i in 0..DIMENSION.num_entries {
                if mutable_metadata == 0 {
                    break;
                }
                let rank = mutable_metadata % (1_usize << DIMENSION.num_bits_per_entry);
                if rank != Dimension::uninit_rank() {
                    if needs_drop::<K>() {
                        unsafe {
                            (*self.entry_array.get()).0[i].as_mut_ptr().drop_in_place();
                        }
                    }
                    if needs_drop::<V>() && (!is_frozen || rank == DIMENSION.removed_rank()) {
                        // `self` being frozen means that reachable values have copied to another
                        // leaf, and they should not be dropped here.
                        unsafe {
                            (*self.entry_array.get()).1[i].as_mut_ptr().drop_in_place();
                        }
                    }
                }
                mutable_metadata >>= DIMENSION.num_bits_per_entry;
            }
        }
    }
}

unsafe impl<K: Send, V: Send> Send for Leaf<K, V> {}
unsafe impl<K: Send + Sync, V: Send + Sync> Sync for Leaf<K, V> {}

impl Dimension {
    /// Flags indicating that the [`Leaf`] is unlinked.
    const UNLINKED: usize = 1_usize << (usize::BITS - 3);

    /// Flags indicating that the [`Leaf`] is frozen.
    const FROZEN: usize = 1_usize << (usize::BITS - 2);

    /// Flags indicating that the [`Leaf`] is retired.
    const RETIRED: usize = 1_usize << (usize::BITS - 1);

    /// Checks if the [`Leaf`] is unlinked.
    #[inline]
    const fn unlinked(metadata: usize) -> bool {
        metadata & Self::UNLINKED != 0
    }

    /// Checks if the [`Leaf`] is frozen.
    #[inline]
    const fn frozen(metadata: usize) -> bool {
        metadata & Self::FROZEN != 0
    }

    /// Makes the metadata represent a frozen state.
    #[inline]
    const fn freeze(metadata: usize) -> usize {
        metadata | Self::FROZEN
    }

    /// Updates the metadata to represent a non-frozen state.
    #[inline]
    const fn unfreeze(metadata: usize) -> usize {
        metadata & (!Self::FROZEN)
    }

    /// Checks if the [`Leaf`] is retired.
    #[inline]
    const fn retired(metadata: usize) -> bool {
        metadata & Self::RETIRED != 0
    }

    /// Makes the metadata represent a retired state.
    #[inline]
    const fn retire(metadata: usize) -> usize {
        metadata | Self::RETIRED
    }

    /// Returns a bit mask for an entry.
    #[inline]
    const fn rank_mask(&self, index: usize) -> usize {
        ((1_usize << self.num_bits_per_entry) - 1) << (index * self.num_bits_per_entry)
    }

    /// Returns the rank of an entry.
    #[inline]
    const fn rank(&self, metadata: usize, index: usize) -> usize {
        (metadata >> (index * self.num_bits_per_entry)) % (1_usize << self.num_bits_per_entry)
    }

    /// Returns the uninitialized rank value which is smaller than all the valid rank values.
    #[inline]
    const fn uninit_rank() -> usize {
        0
    }

    /// Returns the removed rank value which is greater than all the valid rank values.
    #[inline]
    const fn removed_rank(&self) -> usize {
        (1_usize << self.num_bits_per_entry) - 1
    }

    /// Augments the rank to the given metadata.
    #[inline]
    const fn augment(&self, metadata: usize, index: usize, rank: usize) -> usize {
        (metadata & (!self.rank_mask(index))) | (rank << (index * self.num_bits_per_entry))
    }
}

/// The maximum number of entries and the number of metadata bits per entry in a [`Leaf`].
///
/// * `M`: The maximum number of entries.
/// * `B`: The minimum number of bits to express the state of an entry.
/// * `2`: The number of special states of an entry: uninitialized, removed.
/// * `3`: The number of special states of a [`Leaf`]: frozen, retired, and unlinked.
/// * `U`: `usize::BITS`.
/// * `Eq1 = M + 2 <= 2^B`: `B` bits represent at least `M + 2` states.
/// * `Eq2 = B * M + 3 <= U`: `M entries + 3` special state.
/// * `Eq3 = Ceil(Log2(M + 2)) * M + 3 <= U`: derived from `Eq1` and `Eq2`.
///
/// Therefore, when `U = 64 => M = 14 / B = 4`, and `U = 32 => M = 7 / B = 4`.
pub const DIMENSION: Dimension = match usize::BITS / 8 {
    1 => Dimension {
        num_entries: 2,
        num_bits_per_entry: 2,
    },
    2 => Dimension {
        num_entries: 4,
        num_bits_per_entry: 3,
    },
    4 => Dimension {
        num_entries: 7,
        num_bits_per_entry: 4,
    },
    8 => Dimension {
        num_entries: 14,
        num_bits_per_entry: 4,
    },
    _ => Dimension {
        num_entries: 25,
        num_bits_per_entry: 5,
    },
};

impl<'l, K, V> Iter<'l, K, V> {
    /// Creates a new [`Iter`].
    #[inline]
    pub(super) fn new(leaf: &'l Leaf<K, V>) -> Iter<'l, K, V> {
        let metadata = leaf.metadata.load(Acquire);
        Self::with_metadata(leaf, metadata)
    }

    /// Clones the iterator.
    #[inline]
    pub(super) const fn clone(&self) -> Iter<'l, K, V> {
        Iter { ..*self }
    }

    /// Rewinds the iterator to the beginning.
    #[inline]
    pub(super) const fn rewind(&mut self) {
        self.rank = 0;
    }

    /// Converts itself into a [`RevIter`].
    #[inline]
    pub(super) const fn rev(self) -> RevIter<'l, K, V> {
        // `DIMENSION.num_entries - (self.rev_rank as usize) == (self.rank as usize) - 1`.
        #[allow(clippy::cast_possible_truncation)]
        let rev_rank = if self.rank == 0 {
            0
        } else {
            DIMENSION.num_entries as u8 + 1 - self.rank
        };
        RevIter {
            leaf: self.leaf,
            index: self.index,
            metadata: self.metadata,
            rev_rank,
        }
    }

    /// Returns the snapshot of leaf metadata that the [`Iter`] took.
    #[inline]
    pub(super) const fn metadata(&self) -> usize {
        self.metadata
    }

    /// Returns a reference to the entry that the iterator is currently pointing to.
    #[inline]
    pub(super) const fn get(&self) -> Option<(&'l K, &'l V)> {
        if self.rank == 0 {
            return None;
        }
        let index = self.index[(self.rank as usize) - 1] as usize - 1;
        Some((self.leaf.key_at(index), self.leaf.value_at(index)))
    }

    /// Removes the entry that the iterator is currently pointing to.
    #[inline]
    pub(super) fn remove_unchecked(&self) -> RemoveResult {
        // `self.metadata` cannot be passed to the method as it may be outdated.
        let index = self.index[(self.rank as usize) - 1] as usize - 1;
        self.leaf
            .remove_unchecked(self.leaf.metadata.load(Acquire), index)
    }

    /// Returns a reference to the max key.
    #[inline]
    pub(super) fn max_key(&self) -> Option<&'l K> {
        for i in self.index.iter().rev() {
            if *i != 0 {
                return Some(self.leaf.key_at(*i as usize - 1));
            }
        }
        None
    }

    /// Jumps to the next non-empty leaf.
    #[inline]
    pub(super) fn jump(&self, _guard: &'l Guard) -> Option<Iter<'l, K, V>>
    where
        K: Ord,
    {
        let max_key = self.max_key();
        let mut found_unlinked = false;
        let mut next_leaf = Some(self.leaf);
        while let Some(current_leaf) = next_leaf.take() {
            let next_leaf_ptr = current_leaf.next.load(Acquire);
            let Some(leaf) = (unsafe { next_leaf_ptr.as_ref() }) else {
                break;
            };
            let metadata = leaf.metadata.load(Acquire);
            let mut iter = Iter::with_metadata(leaf, metadata);

            found_unlinked |= Dimension::unlinked(current_leaf.metadata.load(Acquire));
            if found_unlinked {
                // Data race resolution:
                //  - T1:                remove(L1) -> range(L0) ->              traverse(L1)
                //  - T2: unlink(L0) ->                             delete(L0)
                //  - T3:                                                        insertSmall(L1)
                //
                // T1 must not see T3's insertion while it still needs to observe its own deletion.
                // Therefore, keys that are smaller than the max key in the current leaf should be
                // filtered out here.
                while let Some((k, _)) = iter.next() {
                    if max_key.is_none_or(|max| max < k) {
                        return Some(iter);
                    }
                }
            }
            if iter.next().is_some() {
                return Some(iter);
            }
            // Empty leaf: continue.
            next_leaf = Some(leaf);
        }
        None
    }

    /// Creates a new [`Iter`] with the supplied metadata.
    #[inline]
    const fn with_metadata(leaf: &'l Leaf<K, V>, metadata: usize) -> Iter<'l, K, V> {
        let index = Leaf::<K, V>::build_index(metadata);
        Iter {
            leaf,
            metadata,
            index,
            rank: 0,
        }
    }
}

impl<K, V> Debug for Iter<'_, K, V> {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Iter")
            .field("metadata", &self.metadata)
            .field("rank", &self.rank)
            .finish()
    }
}

impl<'l, K, V> Iterator for Iter<'l, K, V> {
    type Item = (&'l K, &'l V);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            self.rank += 1;
            if (self.rank as usize) > DIMENSION.num_entries {
                self.rank = 0;
                return None;
            }
            let index = self.index[(self.rank as usize) - 1] as usize;
            if index != 0 {
                return Some((self.leaf.key_at(index - 1), self.leaf.value_at(index - 1)));
            }
        }
    }
}

impl<'l, K, V> RevIter<'l, K, V> {
    /// Creates a new [`RevIter`].
    #[inline]
    pub(super) fn new(leaf: &'l Leaf<K, V>) -> RevIter<'l, K, V> {
        let metadata = leaf.metadata.load(Acquire);
        Self::with_metadata(leaf, metadata)
    }

    /// Rewinds the iterator to the beginning.
    #[inline]
    pub(super) const fn rewind(&mut self) {
        self.rev_rank = 0;
    }

    /// Converts itself into an [`Iter`].
    #[inline]
    pub(super) const fn rev(self) -> Iter<'l, K, V> {
        // `DIMENSION.num_entries - (self.rev_rank as usize) == (self.rank as usize) - 1`.
        #[allow(clippy::cast_possible_truncation)]
        let rank = if self.rev_rank == 0 {
            0
        } else {
            DIMENSION.num_entries as u8 + 1 - self.rev_rank
        };

        Iter {
            leaf: self.leaf,
            metadata: self.metadata,
            index: self.index,
            rank,
        }
    }

    /// Returns the snapshot of leaf metadata that the [`RevIter`] took.
    #[inline]
    pub(super) const fn metadata(&self) -> usize {
        self.metadata
    }

    /// Returns a reference to the entry that the iterator is currently pointing to.
    #[inline]
    pub(super) const fn get(&self) -> Option<(&'l K, &'l V)> {
        if self.rev_rank == 0 {
            return None;
        }
        let index = self.index[DIMENSION.num_entries - (self.rev_rank as usize)] as usize - 1;
        Some((self.leaf.key_at(index), self.leaf.value_at(index)))
    }

    /// Returns a reference to the min key entry.
    #[inline]
    pub(super) fn min_key(&self) -> Option<&'l K> {
        for i in self.index {
            if i != 0 {
                return Some(self.leaf.key_at(i as usize - 1));
            }
        }
        None
    }

    /// Jumps to the prev non-empty leaf.
    #[inline]
    pub(super) fn jump(&self, _guard: &'l Guard) -> Option<RevIter<'l, K, V>>
    where
        K: Ord,
    {
        let min_key = self.min_key();
        let mut prev_leaf = Some(self.leaf);
        let mut found_unlinked = false;
        while let Some(current_leaf) = prev_leaf.take() {
            let prev_leaf_ptr = current_leaf.prev.load(Acquire);
            let Some(leaf) = (unsafe { prev_leaf_ptr.as_ref() }) else {
                break;
            };
            let metadata = leaf.metadata.load(Acquire);
            let mut iter = RevIter::with_metadata(leaf, metadata);

            found_unlinked |= Dimension::unlinked(current_leaf.metadata.load(Acquire));
            if found_unlinked {
                // See `Iter::jump`.
                while let Some((k, _)) = iter.next() {
                    if min_key.is_none_or(|min| min > k) {
                        return Some(iter);
                    }
                }
            }
            if iter.next().is_some() {
                return Some(iter);
            }
            // Empty leaf: continue.
            prev_leaf = Some(leaf);
        }
        None
    }

    #[inline]
    const fn with_metadata(leaf: &'l Leaf<K, V>, metadata: usize) -> RevIter<'l, K, V> {
        let index = Leaf::<K, V>::build_index(metadata);
        RevIter {
            leaf,
            metadata,
            index,
            rev_rank: 0,
        }
    }
}

impl<K, V> Debug for RevIter<'_, K, V> {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RevIter")
            .field("rev_rank", &self.rev_rank)
            .finish()
    }
}

impl<'l, K, V> Iterator for RevIter<'l, K, V> {
    type Item = (&'l K, &'l V);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            self.rev_rank += 1;
            if (self.rev_rank as usize) > DIMENSION.num_entries {
                self.rev_rank = 0;
                return None;
            }
            let index = self.index[DIMENSION.num_entries - (self.rev_rank as usize)] as usize;
            if index != 0 {
                return Some((self.leaf.key_at(index - 1), self.leaf.value_at(index - 1)));
            }
        }
    }
}

#[cfg(not(feature = "loom"))]
#[cfg(test)]
mod test {
    use super::*;
    use proptest::prelude::*;
    use sdd::Shared;
    use std::sync::atomic::AtomicBool;
    use tokio::sync::Barrier;

    #[test]
    fn basic() {
        let leaf: Leaf<String, String> = Leaf::new();
        assert!(matches!(
            leaf.insert("MY GOODNESS!".to_owned(), "OH MY GOD!!".to_owned()),
            InsertResult::Success
        ));
        assert!(matches!(
            leaf.insert("GOOD DAY".to_owned(), "OH MY GOD!!".to_owned()),
            InsertResult::Success
        ));
        assert_eq!(leaf.search_entry("MY GOODNESS!").unwrap().1, "OH MY GOD!!");
        assert_eq!(leaf.search_entry("GOOD DAY").unwrap().1, "OH MY GOD!!");

        for i in 0..DIMENSION.num_entries {
            if let InsertResult::Full(k, v) = leaf.insert(i.to_string(), i.to_string()) {
                assert_eq!(i + 2, DIMENSION.num_entries);
                assert_eq!(k, i.to_string());
                assert_eq!(v, i.to_string());
                break;
            }
            assert_eq!(
                leaf.search_entry(&i.to_string()).unwrap(),
                (&i.to_string(), &i.to_string())
            );
        }

        for i in 0..DIMENSION.num_entries {
            let result = leaf.remove_if(&i.to_string(), &mut |_| i >= 10);
            if i >= 10 && i + 2 < DIMENSION.num_entries {
                assert_eq!(result, RemoveResult::Success);
            } else {
                assert_eq!(result, RemoveResult::Fail);
            }
        }

        assert_eq!(
            leaf.remove_if("GOOD DAY", &mut |v| v == "OH MY"),
            RemoveResult::Fail
        );
        assert_eq!(
            leaf.remove_if("GOOD DAY", &mut |v| v == "OH MY GOD!!"),
            RemoveResult::Success
        );
        assert!(leaf.search_entry("GOOD DAY").is_none());
        assert_eq!(
            leaf.remove_if("MY GOODNESS!", &mut |_| true),
            RemoveResult::Success
        );
        assert!(leaf.search_entry("MY GOODNESS!").is_none());
        assert!(leaf.search_entry("1").is_some());
        assert!(matches!(
            leaf.insert("1".to_owned(), "1".to_owned()),
            InsertResult::Duplicate(..)
        ));
        assert!(matches!(
            leaf.insert("100".to_owned(), "100".to_owned()),
            InsertResult::Full(..)
        ));

        let mut iter = Iter::new(&leaf);
        for i in 0..DIMENSION.num_entries {
            if let Some(e) = iter.next() {
                assert_eq!(e.0, &i.to_string());
                assert_eq!(e.1, &i.to_string());
                assert_ne!(
                    leaf.remove_if(&i.to_string(), &mut |_| true),
                    RemoveResult::Fail
                );
            } else {
                break;
            }
        }

        assert!(matches!(
            leaf.insert("200".to_owned(), "200".to_owned()),
            InsertResult::Full(..)
        ));
    }

    #[test]
    fn iter_rev_iter() {
        let leaf: Leaf<usize, usize> = Leaf::new();
        for i in 0..DIMENSION.num_entries {
            if i % 2 == 0 {
                assert!(matches!(
                    leaf.insert(i * 1024 + 1, i),
                    InsertResult::Success
                ));
            } else {
                assert!(matches!(leaf.insert(i * 2, i), InsertResult::Success));
            }
        }
        assert!(matches!(
            leaf.remove_if(&6, &mut |_| true),
            RemoveResult::Success
        ));

        let mut iter = Iter::new(&leaf);
        assert_eq!(iter.next(), Some((&1, &0)));
        let rev_iter = iter.rev();
        assert_eq!(rev_iter.get(), Some((&1, &0)));
        iter = rev_iter.rev();
        assert_eq!(iter.get(), Some((&1, &0)));

        let mut prev_key = 0;
        let mut sum = 0;
        for (key, _) in Iter::new(&leaf) {
            assert_ne!(*key, 6);
            assert!(prev_key < *key);
            prev_key = *key;
            sum += *key;
        }
        prev_key = usize::MAX;

        for (key, _) in RevIter::new(&leaf) {
            assert_ne!(*key, 6);
            assert!(prev_key > *key);
            prev_key = *key;
            sum -= *key;
        }
        assert_eq!(sum, 0);
    }

    #[test]
    fn calculate_boundary() {
        let leaf: Leaf<usize, usize> = Leaf::new();
        for i in 0..DIMENSION.num_entries {
            assert!(matches!(leaf.insert(i, i), InsertResult::Success));
        }
        assert_eq!(
            Leaf::<usize, usize>::optimal_boundary(leaf.metadata.load(Relaxed)),
            DIMENSION.num_entries - 1
        );

        let leaf: Leaf<usize, usize> = Leaf::new();
        for i in (0..DIMENSION.num_entries).rev() {
            assert!(matches!(leaf.insert(i, i), InsertResult::Success));
        }
        assert_eq!(
            Leaf::<usize, usize>::optimal_boundary(leaf.metadata.load(Relaxed)),
            1
        );

        let leaf: Leaf<usize, usize> = Leaf::new();
        for i in 0..DIMENSION.num_entries {
            if i < DIMENSION.num_entries / 2 {
                assert!(matches!(
                    leaf.insert(usize::MAX - i, usize::MAX - i),
                    InsertResult::Success
                ));
            } else {
                assert!(matches!(leaf.insert(i, i), InsertResult::Success));
            }
        }
        if usize::BITS == 32 {
            assert_eq!(
                Leaf::<usize, usize>::optimal_boundary(leaf.metadata.load(Relaxed)),
                4
            );
        } else {
            assert_eq!(
                Leaf::<usize, usize>::optimal_boundary(leaf.metadata.load(Relaxed)),
                6
            );
        }
    }

    #[test]
    fn special() {
        let leaf: Leaf<usize, usize> = Leaf::new();
        assert!(matches!(leaf.insert(11, 17), InsertResult::Success));
        assert!(matches!(leaf.insert(17, 11), InsertResult::Success));

        let leaf1 = Leaf::new();
        let leaf2 = Leaf::new();
        assert!(leaf.freeze());
        leaf.distribute(|k, v, i, b, _| {
            if i < b {
                leaf1.insert_unchecked(*k, *v, i);
            } else {
                leaf2.insert_unchecked(*k, *v, i - b);
            }
            true
        });
        assert_eq!(leaf1.search_entry(&11), Some((&11, &17)));
        assert_eq!(leaf1.search_entry(&17), Some((&17, &11)));
        assert!(leaf2.is_empty());
        assert!(matches!(leaf.insert(1, 7), InsertResult::Frozen(..)));
        assert_eq!(leaf.remove_if(&17, &mut |_| true), RemoveResult::Frozen);
        assert!(matches!(leaf.insert(3, 5), InsertResult::Frozen(..)));

        assert!(leaf.unfreeze());
        assert!(matches!(leaf.insert(1, 7), InsertResult::Success));

        assert_eq!(leaf.remove_if(&1, &mut |_| true), RemoveResult::Success);
        assert_eq!(leaf.remove_if(&17, &mut |_| true), RemoveResult::Success);
        assert_eq!(leaf.remove_if(&11, &mut |_| true), RemoveResult::Retired);

        assert!(matches!(leaf.insert(5, 3), InsertResult::Full(..)));
    }

    proptest! {
        #[cfg_attr(miri, ignore)]
        #[test]
        fn general(insert in 0_usize..DIMENSION.num_entries, remove in 0_usize..DIMENSION.num_entries) {
            let leaf: Leaf<usize, usize> = Leaf::new();
            assert!(leaf.is_empty());
            for i in 0..insert {
                assert!(matches!(leaf.insert(i, i), InsertResult::Success));
            }
            if insert == 0 {
                assert_eq!(leaf.max_key(), None);
                assert!(leaf.is_empty());
            } else {
                assert_eq!(leaf.max_key(), Some(&(insert - 1)));
                assert!(!leaf.is_empty());
            }
            for i in 0..insert {
                assert!(matches!(leaf.insert(i, i), InsertResult::Duplicate(..)));
                assert!(!leaf.is_empty());
                let result = leaf.min_greater_equal(&i);
                assert_eq!(result.0, Some((&i, &i)));
            }
            for i in 0..insert {
                assert_eq!(leaf.search_entry(&i).unwrap(), (&i, &i));
            }
            if insert == DIMENSION.num_entries {
                assert!(matches!(leaf.insert(usize::MAX, usize::MAX), InsertResult::Full(..)));
            }
            for i in 0..remove {
                if i < insert {
                    if i == insert - 1 {
                        assert!(matches!(leaf.remove_if(&i, &mut |_| true), RemoveResult::Retired));
                        for i in 0..insert {
                            assert!(matches!(leaf.insert(i, i), InsertResult::Full(..)));
                        }
                    } else {
                        assert!(matches!(leaf.remove_if(&i, &mut |_| true), RemoveResult::Success));
                    }
                } else {
                    assert!(matches!(leaf.remove_if(&i, &mut |_| true), RemoveResult::Fail));
                    assert!(leaf.is_empty());
                }
            }
        }

        #[cfg_attr(miri, ignore)]
        #[test]
        fn range(start in 0_usize..DIMENSION.num_entries, end in 0_usize..DIMENSION.num_entries) {
            let leaf: Leaf<usize, usize> = Leaf::new();
            for i in 1..DIMENSION.num_entries - 1 {
                prop_assert!(matches!(leaf.insert(i, i), InsertResult::Success));
            }
            leaf.remove_range(&(start..end));
            for i in 1..DIMENSION.num_entries - 1 {
                prop_assert!(leaf.search_entry(&i).is_none() == (start..end).contains(&i));
            }
            prop_assert!(leaf.search_entry(&0).is_none());
            prop_assert!(leaf.search_entry(&(DIMENSION.num_entries - 1)).is_none());
        }
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 16)]
    async fn update() {
        let num_excess = 3;
        let num_tasks = DIMENSION.num_entries + num_excess;
        for _ in 0..256 {
            let barrier = Shared::new(Barrier::new(num_tasks));
            let leaf: Shared<Leaf<usize, usize>> = Shared::new(Leaf::new());
            let full: Shared<AtomicUsize> = Shared::new(AtomicUsize::new(0));
            let retire: Shared<AtomicUsize> = Shared::new(AtomicUsize::new(0));
            let mut task_handles = Vec::with_capacity(num_tasks);
            for t in 1..=num_tasks {
                let barrier_clone = barrier.clone();
                let leaf_clone = leaf.clone();
                let full_clone = full.clone();
                let retire_clone = retire.clone();
                task_handles.push(tokio::spawn(async move {
                    barrier_clone.wait().await;
                    let inserted = match leaf_clone.insert(t, t) {
                        InsertResult::Success => {
                            assert_eq!(leaf_clone.search_entry(&t).unwrap(), (&t, &t));
                            true
                        }
                        InsertResult::Duplicate(_, _) | InsertResult::Frozen(_, _) => {
                            unreachable!();
                        }
                        InsertResult::Full(k, v) => {
                            assert_eq!(k, v);
                            assert_eq!(k, t);
                            full_clone.fetch_add(1, Relaxed);
                            false
                        }
                    };
                    {
                        let mut prev = 0;
                        let mut iter = Iter::new(&leaf_clone);
                        for e in iter.by_ref() {
                            assert_eq!(e.0, e.1);
                            assert!(*e.0 > prev);
                            prev = *e.0;
                        }
                    }

                    barrier_clone.wait().await;
                    assert_eq!((*full_clone).load(Relaxed), num_excess);
                    if inserted {
                        assert_eq!(leaf_clone.search_entry(&t).unwrap(), (&t, &t));
                    }
                    {
                        let iter = Iter::new(&leaf_clone);
                        assert_eq!(iter.count(), DIMENSION.num_entries);
                    }

                    barrier_clone.wait().await;
                    match leaf_clone.remove_if(&t, &mut |_| true) {
                        RemoveResult::Success => assert!(inserted),
                        RemoveResult::Fail => assert!(!inserted),
                        RemoveResult::Frozen => unreachable!(),
                        RemoveResult::Retired => {
                            assert!(inserted);
                            assert_eq!(retire_clone.swap(1, Relaxed), 0);
                        }
                    }
                }));
            }
            for r in futures::future::join_all(task_handles).await {
                assert!(r.is_ok());
            }
        }
    }

    #[cfg_attr(miri, ignore)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 16)]
    async fn durability() {
        let num_tasks = 16_usize;
        let workload_size = 8_usize;
        for _ in 0..16 {
            for k in 0..=workload_size {
                let barrier = Shared::new(Barrier::new(num_tasks));
                let leaf: Shared<Leaf<usize, usize>> = Shared::new(Leaf::new());
                let inserted: Shared<AtomicBool> = Shared::new(AtomicBool::new(false));
                let mut task_handles = Vec::with_capacity(num_tasks);
                for _ in 0..num_tasks {
                    let barrier_clone = barrier.clone();
                    let leaf_clone = leaf.clone();
                    let inserted_clone = inserted.clone();
                    task_handles.push(tokio::spawn(async move {
                        {
                            barrier_clone.wait().await;
                            if let InsertResult::Success = leaf_clone.insert(k, k) {
                                assert!(!inserted_clone.swap(true, Relaxed));
                            }
                        }
                        {
                            barrier_clone.wait().await;
                            for i in 0..workload_size {
                                if i != k {
                                    let _result = leaf_clone.insert(i, i);
                                }
                                assert!(!leaf_clone.is_retired());
                                assert_eq!(leaf_clone.search_entry(&k).unwrap(), (&k, &k));
                            }
                            for i in 0..workload_size {
                                let _result = leaf_clone.remove_if(&i, &mut |v| *v != k);
                                assert_eq!(leaf_clone.search_entry(&k).unwrap(), (&k, &k));
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
