//! Recovering a poisoned lock instead of panicking on it.
use std::sync::{Mutex, MutexGuard};

/// Locks `mutex`, recovering the guard when a previous holder panicked.
///
/// A panic while a guard is held poisons the mutex: `lock()` then returns [`Err`], and unwrapping
/// it turns every later access into a second panic that hides the first. Recovering the guard keeps
/// the launcher running.
///
/// The guarded value may still be in whatever state the panicking thread left it in. That is safe
/// for the uses here (caches, logs, "is this held?" flags, a set of keys) because no value is
/// dropped mid-update by the operations that can panic while holding them. Do not use this for
/// data whose half-written form would corrupt an output; such a lock should still surface its
/// poison.
pub fn recover<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
