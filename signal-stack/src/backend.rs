use std::mem;

use libc::c_int;

pub trait SigHandler: Clone {
    type Data;

    fn ours() -> Self;
    unsafe fn delegate(&self, signum: c_int, data: Self::Data);
    fn install(&self, signum: c_int) -> Self;
    fn detect(signum: c_int) -> Self;
}

use super::stack::our_handler;
pub use handler_impl::PlatformSigHandler;
pub type PlatformSigData = <PlatformSigHandler as SigHandler>::Data;

#[cfg(windows)]
mod handler_impl {
    use super::*;

    const SIG_DFL: libc::sighandler_t = 0;
    const SIG_IGN: libc::sighandler_t = 1;
    const SIG_GET: libc::sighandler_t = 2;

    type SigHandlerPtr = extern "C" fn(c_int);

    extern "C" fn handler_thunk(signum: c_int) {
        our_handler(signum, ());

        // Handler is uninstalled after each call, so reinstall it
        PlatformSigHandler::ours().install(signum);
    }

    #[derive(Clone)]
    pub struct PlatformSigHandler(libc::sighandler_t);

    impl SigHandler for PlatformSigHandler {
        type Data = ();

        fn ours() -> Self {
            Self(unsafe { mem::transmute::<SigHandlerPtr, _>(handler_thunk) })
        }

        unsafe fn delegate(&self, signum: libc::c_int, _data: Self::Data) {
            // Unhandled signal, call previous handler
            if self.0 == SIG_DFL {
                // Default behaviour on windows is always to exit with code 3
                // https://docs.microsoft.com/en-us/cpp/c-runtime-library/reference/signal?view=msvc-160
                libc::_exit(3);
            } else if self.0 != SIG_IGN {
                // Non-default handler, call directly
                mem::transmute::<_, SigHandlerPtr>(self.0)(signum);
            }
        }

        fn install(&self, signum: libc::c_int) -> Self {
            Self(unsafe { libc::signal(signum, self.0) })
        }

        fn detect(signum: libc::c_int) -> Self {
            Self(unsafe { libc::signal(signum, SIG_GET) })
        }
    }
}

#[cfg(not(windows))]
mod handler_impl {
    use super::*;
    use libc::c_void;

    type SigHandlerPtr = extern "C" fn(c_int);
    type SigActionPtr = extern "C" fn(c_int, *mut libc::siginfo_t, *mut c_void);

    extern "C" fn handler_thunk(signum: c_int, info: *mut libc::siginfo_t, ucontext: *mut c_void) {
        our_handler(signum, (info, ucontext));
    }

    #[derive(Clone)]
    pub struct PlatformSigHandler(libc::sigaction);

    impl SigHandler for PlatformSigHandler {
        type Data = (*mut libc::siginfo_t, *mut c_void);

        fn ours() -> Self {
            Self(unsafe {
                let mut res: libc::sigaction = mem::zeroed();
                res.sa_sigaction = mem::transmute::<SigActionPtr, _>(handler_thunk);
                res.sa_flags = libc::SA_SIGINFO | libc::SA_NOCLDSTOP | libc::SA_RESTART;
                libc::sigfillset(&mut res.sa_mask);
                res
            })
        }

        unsafe fn delegate(&self, signum: libc::c_int, data: Self::Data) {
            if self.0.sa_sigaction == libc::SIG_DFL {
                // Default handler. We want to re-raise the signal, but doing so is racy,
                // so avoid doing it when we think we can replicate what the default handler
                // would do anyway.
                //
                // https://en.wikipedia.org/wiki/Signal_(IPC)#Default_action
                match signum {
                    // Do nothing
                    libc::SIGCHLD | libc::SIGCONT | libc::SIGURG | libc::SIGWINCH => {}
                    // Stop
                    libc::SIGTSTP | libc::SIGTTIN | libc::SIGTTOU => {
                        libc::raise(libc::SIGSTOP);
                    }
                    // Abort
                    libc::SIGABRT
                    | libc::SIGBUS
                    | libc::SIGFPE
                    | libc::SIGILL
                    | libc::SIGQUIT
                    | libc::SIGSEGV
                    | libc::SIGSYS
                    | libc::SIGTRAP
                    | libc::SIGXCPU
                    | libc::SIGXFSZ => libc::abort(),
                    // Terminate
                    libc::SIGALRM
                    | libc::SIGHUP
                    | libc::SIGINT
                    | libc::SIGPIPE
                    | libc::SIGPOLL
                    | libc::SIGPROF
                    | libc::SIGTERM
                    | libc::SIGUSR1
                    | libc::SIGUSR2
                    | libc::SIGVTALRM => libc::_exit(3),
                    _ => {
                        let prev = self.install(signum);
                        libc::raise(signum);
                        if prev.install(signum).0.sa_sigaction != self.0.sa_sigaction {
                            // Uh oh... Race condition! Just set our signal handler again.
                            Self::ours().install(signum);
                        }
                    }
                }
            } else if self.0.sa_sigaction != libc::SIG_IGN {
                // Non-default handler, call directly
                if self.0.sa_flags & libc::SA_SIGINFO != 0 {
                    mem::transmute::<_, SigActionPtr>(self.0.sa_sigaction)(signum, data.0, data.1);
                } else {
                    mem::transmute::<_, SigHandlerPtr>(self.0.sa_sigaction)(signum);
                }
            }
        }

        fn install(&self, signum: libc::c_int) -> Self {
            Self(unsafe {
                let mut res = mem::zeroed();
                libc::sigaction(signum, &self.0, &mut res);
                res
            })
        }

        fn detect(signum: libc::c_int) -> Self {
            Self(unsafe {
                let mut res = mem::zeroed();
                libc::sigaction(signum, std::ptr::null(), &mut res);
                res
            })
        }
    }
}
