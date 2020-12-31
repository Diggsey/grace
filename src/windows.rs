use winapi::shared::minwindef::{BOOL, DWORD};
use winapi::um::consoleapi::SetConsoleCtrlHandler;
use winapi::um::wincon::{
    CTRL_BREAK_EVENT, CTRL_CLOSE_EVENT, CTRL_C_EVENT, CTRL_LOGOFF_EVENT, CTRL_SHUTDOWN_EVENT,
    PHANDLER_ROUTINE,
};

use super::ShutdownType;

pub unsafe fn enter_outer() {}
pub unsafe fn leave_outer() {}

pub type InternalGuard = PHANDLER_ROUTINE;

unsafe extern "system" fn handle_interrupt(ctrl_type: DWORD) -> BOOL {
    match ctrl_type {
        CTRL_C_EVENT | CTRL_BREAK_EVENT => {
            super::handle(ShutdownType::Interrupt);
            1
        }
        _ => 0,
    }
}

unsafe extern "system" fn handle_terminate(ctrl_type: DWORD) -> BOOL {
    match ctrl_type {
        CTRL_CLOSE_EVENT | CTRL_LOGOFF_EVENT | CTRL_SHUTDOWN_EVENT => {
            super::handle(ShutdownType::Terminate);
            1
        }
        _ => 0,
    }
}

pub unsafe fn enter(type_: ShutdownType) -> InternalGuard {
    let handler = Some(match type_ {
        ShutdownType::Interrupt => handle_interrupt,
        ShutdownType::Terminate => handle_terminate,
    });
    SetConsoleCtrlHandler(handler, 1);
    handler
}
pub unsafe fn leave(guard: InternalGuard) {
    SetConsoleCtrlHandler(guard, 0);
}
