use std::sync::Arc;

use libc::c_int;

mod backend;
mod signal_safe;
mod stack;

pub use stack::Handler;
pub unsafe trait SafeHandler: Into<Arc<dyn Handler>> {}

#[derive(Debug)]
pub struct SignalHandlerGuard<'a> {
    signums: &'a [c_int],
    handler_id: stack::HandlerId,
}

impl<'a> SignalHandlerGuard<'a> {
    pub unsafe fn new_unsafe(signums: &'a [c_int], handler: Arc<dyn Handler>) -> Self {
        Self {
            signums,
            handler_id: stack::add_handler(signums, handler),
        }
    }
    pub fn new<H: SafeHandler>(signums: &'a [c_int], handler: H) -> Self {
        unsafe { Self::new_unsafe(signums, handler.into()) }
    }
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
