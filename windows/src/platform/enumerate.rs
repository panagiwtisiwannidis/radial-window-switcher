use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::Arc;

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, BOOL, HWND, LPARAM, TRUE};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumChildWindows, EnumWindows, GetAncestor, GetClassNameW, GetForegroundWindow, GetWindow,
    GetWindowLongW, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsIconic,
    IsWindowVisible, GA_ROOTOWNER, GWL_EXSTYLE, GW_OWNER, WS_EX_APPWINDOW, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW,
};

use super::icons;
use super::util::{from_wide, wide};
use radial_core::model::{AppWindows, Icon, WindowRef};

const HIDDEN_CLASSES: &[&str] = &[
    "Progman",
    "WorkerW",
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
    "Windows.UI.Core.CoreWindow",
];

thread_local! {
    static APPS: RefCell<HashMap<String, (String, Option<Icon>)>> = RefCell::new(HashMap::new());
}

pub struct Snapshot {
    pub front: Option<AppWindows>,
    pub others: Vec<AppWindows>,
}

pub fn snapshot(own_pid: u32) -> Snapshot {
    let mut handles: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(collect), LPARAM(&mut handles as *mut _ as isize));
    }
    let foreground = unsafe { GetAncestor(GetForegroundWindow(), GA_ROOTOWNER) };

    let mut order: Vec<String> = Vec::new();
    let mut groups: HashMap<String, AppWindows> = HashMap::new();
    let mut front_key = None;
    for hwnd in handles {
        if !unsafe { is_switchable(hwnd) } {
            continue;
        }
        let (pid, path) = unsafe { owner(hwnd) };
        if pid == own_pid {
            continue;
        }
        let key = path
            .clone()
            .unwrap_or_else(|| format!("pid:{pid}"))
            .to_lowercase();
        if hwnd == foreground {
            front_key = Some(key.clone());
        }
        let window = WindowRef {
            handle: hwnd.0 as isize,
            pid,
            title: unsafe { title(hwnd) },
            minimized: unsafe { IsIconic(hwnd).as_bool() },
        };
        let group = groups.entry(key.clone()).or_insert_with(|| {
            order.push(key.clone());
            let (name, icon) = app_info(&key, path.as_deref(), hwnd);
            AppWindows {
                name,
                icon,
                windows: Vec::new(),
            }
        });
        group.windows.push(window);
    }

    let front = front_key.as_ref().and_then(|k| groups.remove(k));
    let others = order
        .into_iter()
        .filter_map(|k| groups.remove(&k))
        .collect();
    Snapshot { front, others }
}

unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
    (*(lparam.0 as *mut Vec<HWND>)).push(hwnd);
    TRUE
}

pub(super) unsafe fn is_switchable(hwnd: HWND) -> bool {
    if !IsWindowVisible(hwnd).as_bool() || GetWindowTextLengthW(hwnd) == 0 {
        return false;
    }
    let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
    let app_window = ex_style & WS_EX_APPWINDOW.0 != 0;
    if !app_window && (ex_style & WS_EX_TOOLWINDOW.0 != 0 || ex_style & WS_EX_NOACTIVATE.0 != 0) {
        return false;
    }
    if !app_window && GetWindow(hwnd, GW_OWNER).is_ok_and(|owner| !owner.is_invalid()) {
        return false;
    }
    let mut cloaked = 0u32;
    let cloak_query = DwmGetWindowAttribute(
        hwnd,
        DWMWA_CLOAKED,
        &mut cloaked as *mut _ as *mut c_void,
        4,
    );
    if cloak_query.is_ok() && cloaked != 0 {
        return false;
    }
    !HIDDEN_CLASSES.contains(&class_name(hwnd).as_str())
}

unsafe fn class_name(hwnd: HWND) -> String {
    let mut buffer = [0u16; 256];
    let length = GetClassNameW(hwnd, &mut buffer);
    String::from_utf16_lossy(&buffer[..length.max(0) as usize])
}

pub(super) unsafe fn title(hwnd: HWND) -> String {
    let mut buffer = vec![0u16; GetWindowTextLengthW(hwnd) as usize + 1];
    let length = GetWindowTextW(hwnd, &mut buffer);
    String::from_utf16_lossy(&buffer[..length.max(0) as usize])
}

unsafe fn owner(hwnd: HWND) -> (u32, Option<String>) {
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    let path = process_path(pid);
    let hosted = path
        .as_deref()
        .is_some_and(|p| p.to_lowercase().ends_with("applicationframehost.exe"));
    if hosted {
        let mut search = (pid, 0u32);
        let _ = EnumChildWindows(
            hwnd,
            Some(find_hosted_app),
            LPARAM(&mut search as *mut _ as isize),
        );
        if search.1 != 0 {
            return (search.1, process_path(search.1));
        }
    }
    (pid, path)
}

unsafe extern "system" fn find_hosted_app(child: HWND, lparam: LPARAM) -> BOOL {
    let search = &mut *(lparam.0 as *mut (u32, u32));
    let mut pid = 0u32;
    GetWindowThreadProcessId(child, Some(&mut pid));
    if pid != search.0 {
        search.1 = pid;
        return false.into();
    }
    TRUE
}

pub(super) fn process_path(pid: u32) -> Option<String> {
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buffer = [0u16; 1024];
        let mut length = buffer.len() as u32;
        let result = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut length,
        );
        let _ = CloseHandle(process);
        result.ok()?;
        Some(String::from_utf16_lossy(&buffer[..length as usize]))
    }
}

fn app_info(key: &str, path: Option<&str>, hwnd: HWND) -> (String, Option<Icon>) {
    APPS.with(|apps| {
        apps.borrow_mut()
            .entry(key.to_string())
            .or_insert_with(|| {
                let name = path
                    .and_then(file_description)
                    .or_else(|| path.map(file_stem))
                    .unwrap_or_else(|| unsafe { title(hwnd) });
                let icon = path
                    .and_then(icons::for_executable)
                    .or_else(|| icons::for_window(hwnd))
                    .map(Arc::new);
                (name, icon)
            })
            .clone()
    })
}

fn file_stem(path: &str) -> String {
    std::path::Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn file_description(path: &str) -> Option<String> {
    let path = wide(path);
    unsafe {
        let size = GetFileVersionInfoSizeW(PCWSTR(path.as_ptr()), None);
        if size == 0 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        GetFileVersionInfoW(
            PCWSTR(path.as_ptr()),
            0,
            size,
            data.as_mut_ptr() as *mut c_void,
        )
        .ok()?;

        let mut pointer: *mut c_void = std::ptr::null_mut();
        let mut length = 0u32;
        let translation = wide("\\VarFileInfo\\Translation");
        if !VerQueryValueW(
            data.as_ptr() as *const c_void,
            PCWSTR(translation.as_ptr()),
            &mut pointer,
            &mut length,
        )
        .as_bool()
            || length < 4
        {
            return None;
        }
        let codes = pointer as *const u16;
        let key = format!(
            "\\StringFileInfo\\{:04x}{:04x}\\FileDescription",
            *codes,
            *codes.add(1)
        );
        let key = wide(&key);
        if !VerQueryValueW(
            data.as_ptr() as *const c_void,
            PCWSTR(key.as_ptr()),
            &mut pointer,
            &mut length,
        )
        .as_bool()
            || length == 0
        {
            return None;
        }
        let text = std::slice::from_raw_parts(pointer as *const u16, length as usize);
        let description = from_wide(text).trim().to_string();
        (!description.is_empty()).then_some(description)
    }
}
