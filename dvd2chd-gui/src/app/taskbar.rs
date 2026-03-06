//! Windows taskbar progress via raw COM (ITaskbarList3).
//! No external crates required — uses direct FFI to ole32 and user32.

use std::ffi::c_void;
use std::ptr;

#[allow(clippy::upper_case_acronyms)]
type HRESULT = i32;
#[allow(clippy::upper_case_acronyms)]
type HWND = isize;

#[allow(clippy::upper_case_acronyms)]
#[repr(C)]
struct GUID {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
}

const CLSID_TASKBAR_LIST: GUID = GUID {
    data1: 0x56fdf344,
    data2: 0xfd6d,
    data3: 0x11d0,
    data4: [0x95, 0x8a, 0x00, 0x60, 0x97, 0xc9, 0xa0, 0x90],
};

const IID_ITASKBAR_LIST3: GUID = GUID {
    data1: 0xea1afb91,
    data2: 0x9e28,
    data3: 0x4b86,
    data4: [0x90, 0xe9, 0x9e, 0x9f, 0x8a, 0x5e, 0xef, 0xaf],
};

const CLSCTX_ALL: u32 = 23;
const COINIT_APARTMENTTHREADED: u32 = 2;
const TBPF_NOPROGRESS: u32 = 0x0;
const TBPF_NORMAL: u32 = 0x2;

#[link(name = "ole32")]
extern "system" {
    fn CoInitializeEx(reserved: *mut c_void, coinit: u32) -> HRESULT;
    fn CoCreateInstance(
        rclsid: *const GUID,
        punk_outer: *mut c_void,
        cls_context: u32,
        riid: *const GUID,
        ppv: *mut *mut c_void,
    ) -> HRESULT;
}

#[link(name = "user32")]
extern "system" {
    fn FindWindowW(class_name: *const u16, window_name: *const u16) -> HWND;
}

/// Thin wrapper around the COM ITaskbarList3 interface.
pub struct TaskbarProgress {
    obj: *mut c_void,
    hwnd: HWND,
}

impl TaskbarProgress {
    /// Try to create a taskbar progress handle for the window with the given title.
    pub fn new(title: &str) -> Option<Self> {
        unsafe {
            // COM is usually already initialized by the GUI framework;
            // calling again with the same apartment type is harmless (returns S_FALSE).
            CoInitializeEx(ptr::null_mut(), COINIT_APARTMENTTHREADED);

            let mut obj: *mut c_void = ptr::null_mut();
            let hr = CoCreateInstance(
                &CLSID_TASKBAR_LIST,
                ptr::null_mut(),
                CLSCTX_ALL,
                &IID_ITASKBAR_LIST3,
                &mut obj,
            );
            if hr < 0 || obj.is_null() {
                return None;
            }

            // Call ITaskbarList::HrInit (vtable index 3)
            let vt = Self::vtable(obj);
            type HrInitFn = unsafe extern "system" fn(*mut c_void) -> HRESULT;
            let hr_init: HrInitFn = std::mem::transmute(*vt.add(3));
            hr_init(obj);

            // Find the window by title
            let title_wide: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
            let hwnd = FindWindowW(ptr::null(), title_wide.as_ptr());
            if hwnd == 0 {
                type ReleaseFn = unsafe extern "system" fn(*mut c_void) -> u32;
                let release: ReleaseFn = std::mem::transmute(*vt.add(2));
                release(obj);
                return None;
            }

            Some(TaskbarProgress { obj, hwnd })
        }
    }

    /// Set taskbar progress (0.0 – 1.0).
    pub fn set_progress(&self, value: f32) {
        unsafe {
            let vt = Self::vtable(self.obj);
            let completed = (value.clamp(0.0, 1.0) * 1000.0) as u64;

            // SetProgressValue — vtable index 9
            type SetValueFn = unsafe extern "system" fn(*mut c_void, HWND, u64, u64) -> HRESULT;
            let f: SetValueFn = std::mem::transmute(*vt.add(9));
            f(self.obj, self.hwnd, completed, 1000);

            // SetProgressState — vtable index 10
            type SetStateFn = unsafe extern "system" fn(*mut c_void, HWND, u32) -> HRESULT;
            let f: SetStateFn = std::mem::transmute(*vt.add(10));
            f(self.obj, self.hwnd, TBPF_NORMAL);
        }
    }

    /// Clear taskbar progress (remove the green bar).
    pub fn clear(&self) {
        unsafe {
            let vt = Self::vtable(self.obj);
            type SetStateFn = unsafe extern "system" fn(*mut c_void, HWND, u32) -> HRESULT;
            let f: SetStateFn = std::mem::transmute(*vt.add(10));
            f(self.obj, self.hwnd, TBPF_NOPROGRESS);
        }
    }

    unsafe fn vtable(obj: *mut c_void) -> *const *const c_void {
        *(obj as *const *const *const c_void)
    }
}

impl Drop for TaskbarProgress {
    fn drop(&mut self) {
        unsafe {
            let vt = Self::vtable(self.obj);
            type ReleaseFn = unsafe extern "system" fn(*mut c_void) -> u32;
            let release: ReleaseFn = std::mem::transmute(*vt.add(2));
            release(self.obj);
        }
    }
}
