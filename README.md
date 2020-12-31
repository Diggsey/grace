# grace

Portable library for intercepting various kinds of shutdown signal,
allowing your application to shutdown gracefully.

Windows does not have signals (although they are emulated to some
extent by `libc`) so this crate uses the appropriate windows API
functions directly to respond to interrupt and shutdown requests.

On other platforms signals are used via the `signal-stack`
crate.
