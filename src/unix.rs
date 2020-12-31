use std::cell::UnsafeCell;
use std::mem::MaybeUninit;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;

use signal_stack::SignalHandlerGuard;

use super::ShutdownType;

static mut NOTIFY_SEM: UnsafeCell<MaybeUninit<libc::sem_t>> =
    UnsafeCell::new(MaybeUninit::uninit());
static mut STOP_SEM: UnsafeCell<MaybeUninit<libc::sem_t>> = UnsafeCell::new(MaybeUninit::uninit());
static INT_COUNT: AtomicUsize = AtomicUsize::new(0);
static TERM_COUNT: AtomicUsize = AtomicUsize::new(0);
static STOPPING: AtomicBool = AtomicBool::new(false);

fn load_and_reset(counter: &AtomicUsize) -> usize {
    let res = counter.load(Ordering::Relaxed);
    counter.fetch_sub(res, Ordering::Relaxed);
    res
}

fn sem_ptr(sem: &UnsafeCell<MaybeUninit<libc::sem_t>>) -> *mut libc::sem_t {
    sem.get() as *mut libc::sem_t
}

fn background_thread() {
    unsafe {
        while !STOPPING.load(Ordering::Relaxed) {
            libc::sem_wait(sem_ptr(&NOTIFY_SEM));
            let int_count = load_and_reset(&INT_COUNT);
            let term_count = load_and_reset(&TERM_COUNT);
            for _ in 0..int_count {
                super::handle(ShutdownType::Interrupt);
            }
            for _ in 0..term_count {
                super::handle(ShutdownType::Terminate);
            }
        }
        STOPPING.store(false, Ordering::Relaxed);
        libc::sem_post(sem_ptr(&STOP_SEM));
    }
}

fn signal_handler(signum: libc::c_int) -> bool {
    match signum {
        libc::SIGINT => &INT_COUNT,
        libc::SIGTERM => &TERM_COUNT,
        _ => unreachable!(),
    }
    .fetch_add(1, Ordering::Relaxed);
    unsafe {
        libc::sem_post(sem_ptr(&NOTIFY_SEM));
    }
    true
}

pub unsafe fn enter_outer() {
    libc::sem_init(sem_ptr(&NOTIFY_SEM), 0, 0);
    libc::sem_init(sem_ptr(&STOP_SEM), 0, 0);
    thread::spawn(background_thread);
}

pub type InternalGuard = SignalHandlerGuard<'static>;

pub unsafe fn enter(type_: ShutdownType) -> InternalGuard {
    SignalHandlerGuard::new_unsafe(
        match type_ {
            ShutdownType::Interrupt => &[libc::SIGINT],
            ShutdownType::Terminate => &[libc::SIGTERM],
        },
        Arc::new(signal_handler),
    )
}
pub unsafe fn leave(_guard: InternalGuard) {}

pub unsafe fn leave_outer() {
    STOPPING.store(true, Ordering::Relaxed);
    libc::sem_post(sem_ptr(&NOTIFY_SEM));
    libc::sem_wait(sem_ptr(&STOP_SEM));
    libc::sem_destroy(sem_ptr(&NOTIFY_SEM));
    libc::sem_destroy(sem_ptr(&STOP_SEM));
}
