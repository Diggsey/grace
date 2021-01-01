use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;

use libc::c_int;

use super::backend::{PlatformSigData, PlatformSigHandler, SigHandler};
use super::signal_safe::RwLock;

/// This trait is implemented for functions which match the required signature
/// for signal handlers.
///
/// The signal number is passed in as a parameter.
/// The handler should return `true` if the signal was handled, in which case
/// no further action will be taken. If `false` is returned, then the next
/// handler on the stack will be called, or, if there are no more handlers,
/// the default behaviour for the signal will occur.
pub trait Handler: Fn(c_int) -> bool + Send + Sync {}
impl<T: Fn(c_int) -> bool + Send + Sync> Handler for T {}

#[derive(Clone)]
struct Slot {
    stack: Vec<Arc<dyn Handler>>,
    prev: PlatformSigHandler,
}

impl Slot {
    pub fn new(signum: c_int) -> Self {
        Self {
            stack: Vec::new(),
            prev: PlatformSigHandler::detect(signum),
        }
    }
}

type Handlers = HashMap<c_int, Slot>;

#[derive(Clone)]
pub struct HandlerId(Arc<dyn Handler>);

impl Debug for HandlerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("HandlerId { ... }")
    }
}

impl Eq for HandlerId {}
impl PartialEq for HandlerId {
    fn eq(&self, other: &Self) -> bool {
        // Comparing wide pointers has unpredictable results, so compare thin pointers.
        std::ptr::eq(
            Arc::as_ptr(&self.0) as *const (),
            Arc::as_ptr(&other.0) as *const (),
        )
    }
}

static HANDLERS: RwLock<Option<Handlers>> = RwLock::const_new(None, None);

pub(crate) fn our_handler(signum: c_int, data: PlatformSigData) {
    if let Some(handlers) = &*HANDLERS.read() {
        if let Some(slot) = handlers.get(&signum) {
            for item in slot.stack.iter().rev() {
                if item(signum) {
                    return;
                }
            }
            unsafe {
                slot.prev.delegate(signum, data);
            }
        }
    }
}

pub(crate) unsafe fn add_handler(signums: &[c_int], handler: Arc<dyn Handler>) -> HandlerId {
    let handler_id = HandlerId(handler.clone());

    if !signums.is_empty() {
        let mut install_c_handlers = Vec::new();
        {
            let mut guard = HANDLERS.write();
            let handlers = guard.get_or_insert_with(Default::default);
            for &signum in signums {
                handlers
                    .entry(signum)
                    .or_insert_with(|| {
                        install_c_handlers.push(signum);
                        Slot::new(signum)
                    })
                    .stack
                    .push(handler.clone());
            }
        }

        if !install_c_handlers.is_empty() {
            let prevs: Vec<_> = install_c_handlers
                .into_iter()
                .map(|signum| (signum, PlatformSigHandler::ours().install(signum)))
                .collect();

            let mut guard = HANDLERS.write();
            let handlers = guard.as_mut().unwrap();
            for (signum, prev) in prevs {
                handlers.get_mut(&signum).unwrap().prev = prev;
            }
        }
    }

    handler_id
}

pub(crate) unsafe fn remove_handler(signums: &[c_int], handler_id: &HandlerId) {
    if signums.is_empty() {
        return;
    }
    let ptr = Arc::as_ptr(&handler_id.0) as *const ();
    if let Some(handlers) = HANDLERS.write().as_mut() {
        for &signum in signums {
            if let Some(slot) = handlers.get_mut(&signum) {
                if let Some((index, _)) = slot
                    .stack
                    .iter()
                    .enumerate()
                    .rev()
                    .find(|&(_, item)| Arc::as_ptr(item) as *const () == ptr)
                {
                    slot.stack.remove(index);
                }
            }
        }
    }
}
