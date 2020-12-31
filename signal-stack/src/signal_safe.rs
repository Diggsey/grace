use std::mem::ManuallyDrop;
use std::ops::{Deref, DerefMut};

use parking_lot::lock_api::RawMutex;
use parking_lot::{Mutex, MutexGuard};

pub struct RwLock<T> {
    values: [spin::RwLock<T>; 2],
    mutex: Mutex<()>,
}

impl<T> RwLock<T> {
    pub const fn const_new(value1: T, value2: T) -> Self {
        Self {
            values: [spin::RwLock::new(value1), spin::RwLock::new(value2)],
            mutex: Mutex::const_new(RawMutex::INIT, ()),
        }
    }
    pub fn read(&self) -> ReadGuard<'_, T> {
        // This can never block, because we only ever lock one value at a time
        for value in self.values.iter().cycle() {
            if let Some(inner) = value.try_read() {
                return ReadGuard { inner };
            }
        }
        unreachable!()
    }
    pub fn write(&self) -> WriteGuard<'_, T>
    where
        T: Clone,
    {
        let _mutex_guard = self.mutex.lock();
        let inner = ManuallyDrop::new(self.values[1].write());
        let other = &self.values[0];
        WriteGuard {
            _mutex_guard,
            inner,
            other,
        }
    }
}

pub struct ReadGuard<'a, T> {
    inner: spin::RwLockReadGuard<'a, T>,
}
impl<'a, T> Deref for ReadGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.inner.deref()
    }
}

pub struct WriteGuard<'a, T: Clone> {
    _mutex_guard: MutexGuard<'a, ()>,
    inner: ManuallyDrop<spin::RwLockWriteGuard<'a, T>>,
    other: &'a spin::RwLock<T>,
}
impl<'a, T: Clone> Deref for WriteGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.inner.deref()
    }
}
impl<'a, T: Clone> DerefMut for WriteGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        self.inner.deref_mut()
    }
}
impl<'a, T: Clone> Drop for WriteGuard<'a, T> {
    fn drop(&mut self) {
        // Safety: we only call this once here
        let guard = unsafe { ManuallyDrop::take(&mut self.inner) };

        // If `clone` panics, the value won't get committed, but it shouldn't
        // cause any unsafety or deadlocks.
        let value: T = guard.clone();

        // Release the original guard
        drop(guard);

        // Commit the value by also storing it into the other slot
        // Go via an upgradeable read lock to prevent readers from starving us
        *self.other.upgradeable_read().upgrade() = value;
    }
}
