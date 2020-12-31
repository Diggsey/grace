# signal-stack

Low-level library for installing signal handlers. Signal handlers are
modelled as a stack: when a signal is raised, the stack is traversed
from top to bottom, and each signal handler is called in turn.

A signal handler can return `true` to indicate that the signal was
handled. In this case, no further handlers will be called. If no
signal handler returns `true` then the default behaviour for that
signal will occur.
