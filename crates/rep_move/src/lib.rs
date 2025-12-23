//! Iterator that yields N-1 replications followed by the original value.
//!
//! Optimized for expensive-to-clone types by moving the original on the last iteration.

#![no_std]
#![feature(const_destruct)]
#![feature(const_trait_impl)]

use core::{fmt, iter::FusedIterator, marker::Destruct};

/// Replication strategy for `RepMove`.
pub trait Replicator<T> {
    /// Creates a replica with mutable access to the remaining count.
    fn replicate(&mut self, source: &T, remaining: &mut usize) -> T;
}

// Blanket impl for simple replicators
impl<T, F> Replicator<T> for F
where F: FnMut(&T) -> T
{
    #[inline]
    fn replicate(&mut self, source: &T, remaining: &mut usize) -> T {
        let item = self(source);
        *remaining = remaining.saturating_sub(1);
        item
    }
}

// Note: Additional blanket impls for FnMut(&T, usize) -> T and FnMut(&T, &mut usize) -> T
// would conflict with the above. Users needing state awareness should implement Replicator directly
// or use a wrapper type.

/// State-aware replicator wrapper for read-only access to remaining count.
pub struct ReadState<F>(pub F);

impl<T, F> Replicator<T> for ReadState<F>
where F: FnMut(&T, usize) -> T
{
    #[inline]
    fn replicate(&mut self, source: &T, remaining: &mut usize) -> T {
        let item = (self.0)(source, *remaining);
        *remaining = remaining.saturating_sub(1);
        item
    }
}

/// State-aware replicator wrapper for mutable access to remaining count.
pub struct MutState<F>(pub F);

impl<T, F> Replicator<T> for MutState<F>
where F: FnMut(&T, &mut usize) -> T
{
    #[inline]
    fn replicate(&mut self, source: &T, remaining: &mut usize) -> T { (self.0)(source, remaining) }
}

enum State<T, R> {
    Active { source: T, remaining: usize, rep_fn: R },
    Done,
}

/// Iterator yielding N-1 replicas then the original.
///
/// # Examples
///
/// Simple cloning:
/// ```
/// # use core::num::NonZeroUsize;
/// # use rep_move::RepMove;
/// let v = vec![1, 2, 3];
/// let mut iter = RepMove::new(v, Vec::clone, NonZeroUsize::new(3).unwrap());
///
/// assert_eq!(iter.next(), Some(vec![1, 2, 3]));
/// assert_eq!(iter.next(), Some(vec![1, 2, 3]));
/// assert_eq!(iter.next(), Some(vec![1, 2, 3])); // moved
/// ```
///
/// Read-only state awareness:
/// ```
/// # use core::num::NonZeroUsize;
/// # use rep_move::{RepMove, ReadState};
/// let s = String::from("item");
/// let mut iter = RepMove::new(
///     s,
///     ReadState(|s: &String, n| format!("{}-{}", s, n)),
///     NonZeroUsize::new(3).unwrap()
/// );
///
/// assert_eq!(iter.next(), Some("item-2".to_string()));
/// assert_eq!(iter.next(), Some("item-1".to_string()));
/// assert_eq!(iter.next(), Some("item".to_string()));
/// ```
///
/// Full control over iteration:
/// ```
/// # use core::num::NonZeroUsize;
/// # use rep_move::{RepMove, MutState};
/// let v = vec![1, 2, 3];
/// let mut iter = RepMove::new(
///     v,
///     MutState(|v: &Vec<i32>, remaining: &mut usize| {
///         if v.len() > 10 {
///             *remaining = 0; // Stop early for large vectors
///         } else {
///             *remaining = remaining.saturating_sub(1);
///         }
///         v.clone()
///     }),
///     NonZeroUsize::new(5).unwrap()
/// );
/// // Will yield fewer items due to the custom logic
/// ```
pub struct RepMove<T, R: Replicator<T>> {
    state: State<T, R>,
}

impl<T, R: Replicator<T>> RepMove<T, R> {
    /// Creates a new replicating iterator.
    #[inline]
    pub const fn new(source: T, rep_fn: R, count: usize) -> Self
    where
        T: [const] Destruct,
        R: [const] Destruct,
    {
        if count == 0 {
            Self { state: State::Done }
        } else {
            Self { state: State::Active { source, remaining: count - 1, rep_fn } }
        }
    }
}

impl<T, R: Replicator<T>> Iterator for RepMove<T, R> {
    type Item = T;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let state = core::mem::replace(&mut self.state, State::Done);

        match state {
            State::Active { source, mut remaining, mut rep_fn } => {
                if remaining > 0 {
                    let item = rep_fn.replicate(&source, &mut remaining);
                    self.state = State::Active { source, remaining, rep_fn };
                    Some(item)
                } else {
                    Some(source)
                }
            }
            State::Done => None,
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.len();
        (len, Some(len))
    }
}

impl<T, R: Replicator<T>> ExactSizeIterator for RepMove<T, R> {
    #[inline]
    fn len(&self) -> usize {
        match &self.state {
            State::Active { remaining, .. } => remaining + 1,
            State::Done => 0,
        }
    }
}

impl<T, R: Replicator<T>> FusedIterator for RepMove<T, R> {}

impl<T: fmt::Debug, R: Replicator<T>> fmt::Debug for RepMove<T, R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.state {
            State::Active { source, remaining, .. } => f
                .debug_struct("RepMove")
                .field("source", source)
                .field("remaining", remaining)
                .finish_non_exhaustive(),
            State::Done => write!(f, "RepMove::Done"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    extern crate alloc;

    use alloc::{
        format,
        string::{String, ToString as _},
        vec,
        vec::Vec,
    };

    #[test]
    fn test_simple_clone() {
        let v = vec![1, 2, 3];
        let mut iter = RepMove::new(v, Vec::clone, 3);

        assert_eq!(iter.len(), 3);
        assert_eq!(iter.next(), Some(vec![1, 2, 3]));
        assert_eq!(iter.len(), 2);
        assert_eq!(iter.next(), Some(vec![1, 2, 3]));
        assert_eq!(iter.len(), 1);
        assert_eq!(iter.next(), Some(vec![1, 2, 3]));
        assert_eq!(iter.len(), 0);
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn test_state_aware() {
        let s = String::from("test");
        let mut iter = RepMove::new(s, ReadState(|s: &String, n| format!("{}-{}", s, n)), 2);

        assert_eq!(iter.next(), Some("test-1".to_string()));
        assert_eq!(iter.next(), Some("test".to_string()));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn test_mutable_control() {
        let v = vec![1, 2, 3];
        let mut iter = RepMove::new(
            v,
            MutState(|v: &Vec<i32>, remaining: &mut usize| {
                if *remaining > 1 {
                    *remaining = 1; // Skip ahead
                } else {
                    *remaining = remaining.saturating_sub(1);
                }
                v.clone()
            }),
            4,
        );

        // Should yield fewer items due to skipping
        assert_eq!(iter.next(), Some(vec![1, 2, 3]));
        assert_eq!(iter.next(), Some(vec![1, 2, 3]));
        assert_eq!(iter.next(), Some(vec![1, 2, 3]));
        assert_eq!(iter.next(), None);
    }
}
