//! Win32 window styling and process-memory housekeeping.
//!
//! All FFI calls here are best-effort: a failure must never prevent the
//! launcher from opening, so results are deliberately discarded.

use gpui::Window;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Dwm::{
    DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND, DwmExtendFrameIntoClientArea,
    DwmSetWindowAttribute,
};
use windows::Win32::System::ProcessStatus::K32EmptyWorkingSet;
use windows::Win32::System::Threading::GetCurrentProcess;
use windows::Win32::UI::Controls::MARGINS;
use windows::Win32::UI::WindowsAndMessaging::{
    HWND_TOPMOST, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SetForegroundWindow, SetWindowPos,
};

pub(crate) fn setup_window(window: &Window) {
    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };

    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
        return;
    };

    let hwnd = HWND(win32.hwnd.get() as *mut std::ffi::c_void);

    // A 1px extended frame lets DWM draw its hardware drop shadow around
    // an otherwise borderless popup.
    let margins = MARGINS {
        cxLeftWidth: 1,
        cxRightWidth: 1,
        cyTopHeight: 1,
        cyBottomHeight: 1,
    };
    // SAFETY: `hwnd` is the live window handle provided by GPUI and
    // `margins` is a valid initialized struct passed by pointer.
    let _ = unsafe { DwmExtendFrameIntoClientArea(hwnd, &raw const margins) };

    // Square corners match the terminal aesthetic of the UI.
    let corner_pref = DWMWCP_DONOTROUND;
    // SAFETY: the pointer and byte count describe the live `corner_pref`
    // value and match the DWMWA_WINDOW_CORNER_PREFERENCE attribute type.
    let _ = unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            (&raw const corner_pref).cast::<std::ffi::c_void>(),
            u32::try_from(std::mem::size_of_val(&corner_pref)).unwrap_or(u32::MAX),
        )
    };
}

pub(crate) fn make_always_on_top(window: &Window) -> bool {
    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return false;
    };

    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
        return false;
    };

    let hwnd = HWND(win32.hwnd.get() as *mut std::ffi::c_void);
    // SAFETY: `hwnd` is the live window handle supplied by GPUI. The flags
    // leave geometry untouched, so no invalid size/position is passed.
    unsafe {
        let res = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            0x00,
            0x00,
            0x00,
            0x00,
            SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
        );
        let _ = SetForegroundWindow(hwnd);
        res.is_ok()
    }
}

/// Ask Windows to evict this process's idle pages back to standby. This
/// lowers the working set only: committed memory and GPU allocations stay
/// resident, and the pages fault back in when the launcher next opens.
pub(crate) fn trim_working_set() {
    // SAFETY: `GetCurrentProcess` returns a pseudo-handle that requires no
    // closing; `K32EmptyWorkingSet` only touches this process's pages.
    unsafe {
        let _ = K32EmptyWorkingSet(GetCurrentProcess());
    }
}
