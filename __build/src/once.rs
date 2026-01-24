use std::sync::Once;

static ONCE: Once = Once::new();

#[cold]
pub fn initialize<F>(f: F)
where F: FnOnce() {
  ONCE.call_once_force(|_| f());
}

#[inline]
pub fn initialized() -> bool {
  ONCE.is_completed()
}
