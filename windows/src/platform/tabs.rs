use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};

use windows::core::{Result, VARIANT};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationInvokePattern,
    IUIAutomationLegacyIAccessiblePattern, IUIAutomationSelectionItemPattern, TreeScope_Children,
    TreeScope_Descendants, UIA_ControlTypePropertyId, UIA_InvokePatternId,
    UIA_LegacyIAccessiblePatternId, UIA_SelectionItemPatternId, UIA_TabControlTypeId,
    UIA_TabItemControlTypeId,
};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

use super::enumerate;
use super::util::WM_TABS_READY;
use radial_core::model::TabInfo;

const TABBED_APPS: &[&str] = &[
    "msedge.exe",
    "chrome.exe",
    "firefox.exe",
    "brave.exe",
    "opera.exe",
    "vivaldi.exe",
    "arc.exe",
    "windowsterminal.exe",
    "explorer.exe",
    "notepad.exe",
    "code.exe",
];

enum Request {
    Refresh(Vec<(isize, u32)>),
    Select(isize, usize),
}

#[derive(Clone)]
pub struct Tabs {
    requests: Sender<Request>,
    known: Arc<Mutex<HashMap<isize, Vec<TabInfo>>>>,
}

impl Tabs {
    pub fn start(notify: HWND) -> Self {
        let (requests, receiver) = channel::<Request>();
        let known = Arc::new(Mutex::new(HashMap::new()));
        let shared = known.clone();
        let notify = notify.0 as isize;
        std::thread::spawn(move || {
            let Ok(worker) = Worker::new() else { return };
            let mut worker = worker;
            while let Ok(request) = receiver.recv() {
                match request {
                    Request::Refresh(windows) => {
                        let found = worker.refresh(&windows);
                        if let Ok(mut known) = shared.lock() {
                            *known = found;
                        }
                        unsafe {
                            let _ = PostMessageW(
                                HWND(notify as *mut c_void),
                                WM_TABS_READY,
                                WPARAM(0),
                                LPARAM(0),
                            );
                        }
                    }
                    Request::Select(window, index) => worker.select(window, index),
                }
            }
        });
        Self { requests, known }
    }

    pub fn refresh(&self, windows: Vec<(isize, u32)>) {
        let _ = self.requests.send(Request::Refresh(windows));
    }

    pub fn select(&self, window: isize, index: usize) {
        let _ = self.requests.send(Request::Select(window, index));
    }

    pub fn known(&self) -> HashMap<isize, Vec<TabInfo>> {
        self.known.lock().map(|k| k.clone()).unwrap_or_default()
    }
}

struct Worker {
    automation: IUIAutomation,
    elements: HashMap<isize, Vec<IUIAutomationElement>>,
    apps: HashMap<u32, bool>,
}

impl Worker {
    fn new() -> Result<Self> {
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
            let automation: IUIAutomation =
                CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)?;
            Ok(Self {
                automation,
                elements: HashMap::new(),
                apps: HashMap::new(),
            })
        }
    }

    fn is_tabbed(&mut self, pid: u32) -> bool {
        *self.apps.entry(pid).or_insert_with(|| {
            enumerate::process_path(pid)
                .and_then(|path| {
                    std::path::Path::new(&path)
                        .file_name()
                        .map(|n| n.to_string_lossy().to_lowercase())
                })
                .is_some_and(|name| TABBED_APPS.contains(&name.as_str()))
        })
    }

    fn refresh(&mut self, windows: &[(isize, u32)]) -> HashMap<isize, Vec<TabInfo>> {
        let mut found = HashMap::new();
        self.elements.clear();
        for &(window, pid) in windows {
            if !self.is_tabbed(pid) {
                continue;
            }
            if let Ok((tabs, elements)) = self.read(window) {
                if tabs.len() >= 2 {
                    found.insert(window, tabs);
                    self.elements.insert(window, elements);
                }
            }
        }
        found
    }

    fn read(&self, window: isize) -> Result<(Vec<TabInfo>, Vec<IUIAutomationElement>)> {
        unsafe {
            let root = self
                .automation
                .ElementFromHandle(HWND(window as *mut c_void))?;
            let strip_condition = self.automation.CreatePropertyCondition(
                UIA_ControlTypePropertyId,
                &VARIANT::from(UIA_TabControlTypeId.0),
            )?;
            let item_condition = self.automation.CreatePropertyCondition(
                UIA_ControlTypePropertyId,
                &VARIANT::from(UIA_TabItemControlTypeId.0),
            )?;
            let strip = root.FindFirst(TreeScope_Descendants, &strip_condition)?;
            let items = strip.FindAll(TreeScope_Children, &item_condition)?;
            let mut tabs = Vec::new();
            let mut elements = Vec::new();
            for i in 0..items.Length()? {
                let item = items.GetElement(i)?;
                let title = item
                    .CurrentName()
                    .map(|n| n.to_string())
                    .unwrap_or_default();
                let selected = item
                    .GetCurrentPatternAs::<IUIAutomationSelectionItemPattern>(
                        UIA_SelectionItemPatternId,
                    )
                    .and_then(|p| p.CurrentIsSelected())
                    .is_ok_and(|s| s.as_bool());
                tabs.push(TabInfo { title, selected });
                elements.push(item);
            }
            Ok((tabs, elements))
        }
    }

    fn select(&self, window: isize, index: usize) {
        let Some(element) = self.elements.get(&window).and_then(|e| e.get(index)) else {
            return;
        };
        unsafe {
            if let Ok(pattern) = element.GetCurrentPatternAs::<IUIAutomationSelectionItemPattern>(
                UIA_SelectionItemPatternId,
            ) {
                if pattern.Select().is_ok() {
                    return;
                }
            }
            if let Ok(pattern) =
                element.GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId)
            {
                if pattern.Invoke().is_ok() {
                    return;
                }
            }
            if let Ok(pattern) = element
                .GetCurrentPatternAs::<IUIAutomationLegacyIAccessiblePattern>(
                    UIA_LegacyIAccessiblePatternId,
                )
            {
                let _ = pattern.DoDefaultAction();
            }
        }
    }
}
