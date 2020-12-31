//! # signal-stack
//!
//! Low-level library for installing signal handlers. Signal handlers are
//! modelled as a stack: when a signal is raised, the stack is traversed
//! from top to bottom, and each signal handler is called in turn.
//!
//! A signal handler can return `true` to indicate that the signal was
//! handled. In this case, no further handlers will be called. If no
//! signal handler returns `true` then the default behaviour for that
//! signal will occur.

#![deny(missing_docs)]

use std::sync::Arc;

use libc::c_int;

mod backend;
mod signal_safe;
mod stack;

pub use stack::Handler;

/// A type may implement this trait to indicate that it can be converted
/// into an async-signal-safe function. ie. one that is safe to call from
/// a signal handler.
pub unsafe trait SafeHandler: Into<Arc<dyn Handler>> {}

/// This is the primary interface to the crate. When this guard is constructed
/// a new signal handler for one or more signals is pushed onto the top of the
/// stack.
///
/// When it is dropped, the signal handler will be removed from the stack.
/// Signal handlers can be removed at any time, it need not be in reverse
/// order, although that would typically be the case.
#[derive(Debug)]
pub struct SignalHandlerGuard<'a> {
    signums: &'a [c_int],
    handler_id: stack::HandlerId,
}

impl<'a> SignalHandlerGuard<'a> {
    /// Add a new signal handler. The handler function *must* be
    /// async-signal-safe, which places strong restrictions on what the handler
    /// may do.
    ///
    /// A non-exhaustive list of things that are not allowed:
    /// - Allocating or freeing memory.
    /// - Locking or unlocking mutexes or other kinds of concurrency primitive,
    ///   with the exception of posting to a `libc` semaphore.
    /// - Calling a function which is not itself marked as async-signal-safe.
    /// - Performing any kind of blocking I/O.
    pub unsafe fn new_unsafe(signums: &'a [c_int], handler: Arc<dyn Handler>) -> Self {
        Self {
            signums,
            handler_id: stack::add_handler(signums, handler),
        }
    }

    /// Safely construct a signal guard from a function known statically to be
    /// async-signal-safe.
    pub fn new<H: SafeHandler>(signums: &'a [c_int], handler: H) -> Self {
        unsafe { Self::new_unsafe(signums, handler.into()) }
    }

    /// Forget this signal guard: the handler will remain attached for the lifetime
    /// of the program.
    pub fn forget(mut self) {
        self.signums = &[];
    }
}

impl<'a> Drop for SignalHandlerGuard<'a> {
    fn drop(&mut self) {
        unsafe {
            stack::remove_handler(self.signums, &self.handler_id);
        }
    }
}
