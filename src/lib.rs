//! # grace
//!
//! Portable library for intercepting various kinds of shutdown signal,
//! allowing your application to shutdown gracefully.
//!
//! Windows does not have signals (although they are emulated to some
//! extent by `libc`) so this crate uses the appropriate windows API
//! functions directly to respond to interrupt and shutdown requests.
//!
//! On other platforms signals are used via the `signal-stack`
//! crate.
#![deny(missing_docs)]

use std::cell::UnsafeCell;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::mem::ManuallyDrop;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{mpsc, Arc};

use parking_lot::lock_api::RawMutex;
use parking_lot::Mutex;

#[cfg(feature = "futures")]
use futures::channel::mpsc as async_mpsc;
#[cfg(feature = "futures")]
use futures::SinkExt;

static STATE: Mutex<Option<State>> = Mutex::const_new(RawMutex::INIT, None);

/// This crate currently distinguishes two kinds of shutdown request.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ShutdownType {
    /// Program was interrupted via eg. Ctrl + C. This corresponds
    /// to `SIGINT` on unix-based platforms.
    Interrupt,
    /// Program was requested to terminate normally. This corresponds
    /// to `SIGTERM` on unix-based platforms.
    Terminate,
}

/// This trait is implemented for functions which match the required signature
/// for shutdown handlers.
///
/// The shutdown request type is passed in as a parameter.
/// The handler will be called on a background thread, so does not need to be
/// async-signal-safe.
///
/// On windows, this is simply the way the API is designed to work (windows
/// will automatically spawn a thread to handle the shutdown request).
///
/// On other platforms, this crate will spawn a background thread whenever
/// the first `ShutdownGuard` is created, and will stop it whenever the last
/// `ShutdownGuard` is destroyed. Shutdown handlers will run on this
/// background thread.
pub trait Handler: FnMut(ShutdownType) + Send + 'static {}
impl<T: FnMut(ShutdownType) + Send + 'static> Handler for T {}

struct Slot {
    guard: ManuallyDrop<InternalGuard>,
    handlers: Vec<Arc<UnsafeCell<dyn Handler>>>,
}

impl Slot {
    fn new(type_: ShutdownType) -> Self {
        let guard = unsafe { ManuallyDrop::new(enter(type_)) };
        let handlers = Vec::new();
        Self { guard, handlers }
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        unsafe { leave(ManuallyDrop::take(&mut self.guard)) }
    }
}

struct State {
    slots: HashMap<ShutdownType, Slot>,
}

// Our handlers need not be `Sync`, because we never call them concurrently. That said,
// we do need to keep multiple references to them, so we can't use a `Box`.
unsafe impl Send for State {}

impl State {
    fn new() -> Self {
        unsafe {
            enter_outer();
        }
        Self {
            slots: HashMap::new(),
        }
    }
}
impl Drop for State {
    fn drop(&mut self) {
        unsafe {
            leave_outer();
        }
    }
}

fn handle(type_: ShutdownType) {
    let guard = STATE.lock();
    if let Some(state) = guard.as_ref() {
        if let Some(slot) = state.slots.get(&type_) {
            if let Some(handler) = slot.handlers.last() {
                // Safety: We only call the function when we have locked the state mutex,
                // so guaranteed no other accessors.
                let _ = catch_unwind(AssertUnwindSafe(|| unsafe { (*handler.get())(type_) }));
                return;
            }
        }
    }

    // Handler must have been removed, terminate the process
    std::process::exit(3);
}

/// This is the primary interface to the crate.
///
/// Construct an instance of this type to begin intercepting shutdown requests.
/// When the guard is dropped, shutdown requests will no longer be intercepted.
///
/// There are several constructors to simplify common usage patterns.
pub struct ShutdownGuard<'a> {
    types: &'a [ShutdownType],
    handler: Arc<UnsafeCell<dyn Handler>>,
}

impl<'a> ShutdownGuard<'a> {
    /// Call a user-defined function whenever a shutdown is requested.
    pub fn new<H: Handler>(types: &'a [ShutdownType], handler: H) -> Self {
        unsafe { Self::new_inner(types, Arc::new(UnsafeCell::new(handler))) }
    }
    /// Send on an mpsc channel whenever a shutdown is requested.
    pub fn new_channel(types: &'a [ShutdownType]) -> (Self, mpsc::Receiver<ShutdownType>) {
        let (tx, rx) = mpsc::channel();
        (
            Self::new(types, move |t| {
                let _ = tx.send(t);
            }),
            rx,
        )
    }
    /// Send to an async mpsc channel whenever a shutdown is requested.
    #[cfg(feature = "futures")]
    pub fn new_stream(
        types: &'a [ShutdownType],
    ) -> (Self, async_mpsc::UnboundedReceiver<ShutdownType>) {
        let (mut tx, rx) = async_mpsc::unbounded();
        (
            Self::new(types, move |t| {
                let _ = futures::executor::block_on(tx.send(t));
            }),
            rx,
        )
    }
    // Safety: the `Arc` must not be shared elsewhere
    unsafe fn new_inner(types: &'a [ShutdownType], handler: Arc<UnsafeCell<dyn Handler>>) -> Self {
        if !types.is_empty() {
            let mut guard = STATE.lock();
            let state = guard.get_or_insert_with(State::new);
            for &type_ in types {
                state
                    .slots
                    .entry(type_)
                    .or_insert_with(|| Slot::new(type_))
                    .handlers
                    .push(handler.clone());
            }
        }

        Self { types, handler }
    }
    /// Forget this guard, leaving the shutdown handler installed for the
    /// lifetime of the program.
    pub fn forget(mut self) {
        self.types = &[];
    }
}

impl<'a> Drop for ShutdownGuard<'a> {
    fn drop(&mut self) {
        if !self.types.is_empty() {
            let ptr = Arc::as_ptr(&self.handler) as *const ();
            let mut guard = STATE.lock();
            let state = guard.as_mut().expect("State should be initialized");
            for &type_ in self.types {
                if let Entry::Occupied(mut occ) = state.slots.entry(type_) {
                    let handlers = &mut occ.get_mut().handlers;
                    let (index, _) = handlers
                        .iter()
                        .enumerate()
                        .rev()
                        .find(|&(_, item)| Arc::as_ptr(item) as *const () == ptr)
                        .expect("State should contain handler");
                    handlers.remove(index);
                    if handlers.is_empty() {
                        occ.remove();
                    }
                }
            }
            if state.slots.is_empty() {
                guard.take();
            }
        }
    }
}

#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows::*;

#[cfg(not(windows))]
mod unix;
#[cfg(not(windows))]
use unix::*;
