use crate::app::model::{ExtToken, RequestLog, TokenKey};
use alloc::collections::VecDeque;

type HashMap<K, V> = hashbrown::HashMap<K, V, ahash::RandomState>;

pub struct AssociatedToken {
    // pub user: Option<UserProfile>,
    pub token: ExtToken,
    pub ref_count: usize,
    // pub use_pri: bool,
}

pub type MainStorage = VecDeque<RequestLog>;
pub type AssociatedStorage = HashMap<TokenKey, AssociatedToken>;
// pub type SecondaryStorage = HashMap<TokenKey, ExtToken>;

// pub type IndexStorage = Vec<(usize, Hash)>;

// pub trait IndexStorageImpl: Sized {
//     fn get_id(&self, id: &usize) -> Option<Hash>;
// }

// impl IndexStorageImpl for IndexStorage {
//     fn get_id(&self, id: &usize) -> Option<Hash> {
//         let i = match self.binary_search_by(|(probe, _)| probe.cmp(id)) {
//             Ok(n) => n + 1,
//             Err(n) => n,
//         };
//         self.get(i).map(|(_, p)| *p)
//     }
// }
