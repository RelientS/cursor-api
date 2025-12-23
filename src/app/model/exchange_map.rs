type HashMap<K, V> = hashbrown::HashMap<K, V, ahash::RandomState>;
type HashSet<K> = hashbrown::HashSet<K, ahash::RandomState>;

pub struct ExchangeMap {
    cache: HashSet<&'static str>,
    map: HashMap<String, String>,
}

impl Default for ExchangeMap {
    #[inline]
    fn default() -> Self {
        let hasher = ahash::RandomState::new();
        let cache = HashSet::with_hasher(hasher.clone());
        let map = HashMap::with_hasher(hasher);
        Self { cache, map }
    }
}

impl TryFrom<HashMap<String, String>> for ExchangeMap {
    type Error = ();
    #[inline]
    fn try_from(map: HashMap<String, String>) -> Result<Self, ()> {
        let mut set = HashSet::with_capacity_and_hasher(map.len() * 2, map.hasher().clone());
        for (k, v) in &map {
            if !set.insert(k) {
                return Err(());
            }
            if !set.insert(v) {
                return Err(());
            }
        }
        Ok(Self { cache: HashSet::with_hasher(map.hasher().clone()), map })
    }
}

impl ExchangeMap {
    #[inline]
    pub fn resolve(&mut self, path: &'static str) -> &'static str {
        let Some(path) = self.map.get(path) else { self.cache.insert(path); return path };
        self.cache.get_or_insert_with(path.as_str(), |s| {
            let s: Box<str> = Box::from(s);
            Box::leak(s)
        })
    }
    #[inline]
    pub fn finish(self) -> HashSet<&'static str> {
        self.cache
    }
}
