// Capture Context Feedback.
//
// Reports what is currently in front of the user - active application,
// window title, window state, monitor, size/position - so a screen
// reader user can hear a concise summary of what a screenshot or
// recording is actually about to capture, before it happens.
//
// This module only *describes* the current foreground window. It does
// not change which monitor take_native_screenshot() captures (still
// always the primary monitor - see lib.rs) and it makes no claim about
// off-screen, scrolled, or hidden content within a captured window;
// see the "Important Technical Limitation" note in
// docs/Screen Reader First Principles.md.
//
// NOTE: written without a Windows machine or a compiler available in
// this environment. The `windows` crate API shapes below are believed
// correct but have not been built against real crate documentation -
// see docs/Roadmap.md's "What's honestly still open" before treating
// this as verified.

use serde::Serialize;
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFOEXW,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowPlacement, GetWindowRect, GetWindowTextW,
    GetWindowThreadProcessId, MonitorFromWindow, MONITOR_DEFAULTTONEAREST,
    SW_SHOWMAXIMIZED, SW_SHOWMINIMIZED, WINDOWPLACEMENT,
};

/// Windows reports maximized/minimized/normal via GetWindowPlacement's
/// showCmd. "Full screen" (a borderless window exactly covering a
/// monitor - common for games and video players) isn't a distinct
/// showCmd value, so it's inferred by comparing the window's rect to
/// its monitor's rect.
const EDGE_TOLERANCE_PX: i32 = 8; // accounts for the few px of invisible resize border Windows 10/11 windows commonly have

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CaptureContext {
    pub app_name: String,
    pub window_title: String,
    pub state: String,
    pub monitor_number: Option<u32>,
    pub monitor_width: Option<i32>,
    pub monitor_height: Option<i32>,
    pub window_width: Option<i32>,
    pub window_height: Option<i32>,
    pub fills_screen: Option<bool>,
    pub portion: Option<String>,
    pub extends_beyond_monitor: Option<bool>,
    /// Always "entire monitor" today - Phase 2 only ever captures the
    /// whole primary monitor. Kept as a field (rather than hardcoded
    /// in the frontend) so a future active-window or region capture
    /// mode only has to change this value, not the reporting shape.
    pub capture_target: String,
}

fn friendly_app_name(exe_stem: &str) -> String {
    let lower = exe_stem.to_lowercase();
    let known = match lower.as_str() {
        "chrome" => Some("Chrome"),
        "msedge" => Some("Edge"),
        "firefox" => Some("Firefox"),
        "winword" => Some("Word"),
        "excel" => Some("Excel"),
        "powerpnt" => Some("PowerPoint"),
        "outlook" => Some("Outlook"),
        "explorer" => Some("File Explorer"),
        "notepad" => Some("Notepad"),
        "code" => Some("Visual Studio Code"),
        "accessible-screen-capture" | "accessiblescreencapture" => Some("AccessibleScreenCapture"),
        _ => None,
    };

    match known {
        Some(name) => name.to_string(),
        None => {
            // Fall back to a capitalized version of the raw exe name
            // rather than guessing at a product name we don't know.
            let mut chars = exe_stem.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => "Unknown application".to_string(),
            }
        }
    }
}

unsafe fn window_title(hwnd: HWND) -> String {
    let mut buffer = [0u16; 512];
    let length = GetWindowTextW(hwnd, &mut buffer);
    if length <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buffer[..length as usize])
}

unsafe fn process_app_name(hwnd: HWND) -> String {
    let mut pid: u32 = 0;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid == 0 {
        return "Unknown application".to_string();
    }

    let handle = match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
        Ok(handle) => handle,
        Err(_) => return "Unknown application".to_string(),
    };

    let mut buffer = [0u16; 1024];
    let mut size = buffer.len() as u32;
    let result = QueryFullProcessImageNameW(
        handle,
        PROCESS_NAME_WIN32,
        windows::core::PWSTR(buffer.as_mut_ptr()),
        &mut size,
    );

    if result.is_err() {
        return "Unknown application".to_string();
    }

    let path = String::from_utf16_lossy(&buffer[..size as usize]);
    let stem = path
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(&path)
        .trim_end_matches(".exe");

    friendly_app_name(stem)
}

unsafe fn window_show_state(hwnd: HWND) -> u32 {
    let mut placement = WINDOWPLACEMENT {
        length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
        ..Default::default()
    };
    let _ = GetWindowPlacement(hwnd, &mut placement);
    placement.showCmd
}

struct MonitorMatch {
    number: u32,
    rect: RECT,
}

unsafe extern "system" fn count_monitors_proc(
    hmonitor: HMONITOR,
    _hdc: HDC,
    _rect: *mut RECT,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::BOOL {
    let data = &mut *(lparam.0 as *mut (HMONITOR, u32, Option<MonitorMatch>));
    let (target, ref mut count, ref mut found) = *data;
    *count += 1;

    if hmonitor == target {
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        if GetMonitorInfoW(hmonitor, &mut info.monitorInfo as *mut _ as *mut _).as_bool() {
            *found = Some(MonitorMatch {
                number: *count,
                rect: info.monitorInfo.rcMonitor,
            });
        }
    }

    windows::Win32::Foundation::BOOL(1)
}

unsafe fn locate_monitor(hmonitor: HMONITOR) -> Option<MonitorMatch> {
    let mut data: (HMONITOR, u32, Option<MonitorMatch>) = (hmonitor, 0, None);
    let lparam = windows::Win32::Foundation::LPARAM(&mut data as *mut _ as isize);
    let _ = EnumDisplayMonitors(None, None, Some(count_monitors_proc), lparam);
    data.2
}

fn rect_width(rect: &RECT) -> i32 {
    rect.right - rect.left
}
fn rect_height(rect: &RECT) -> i32 {
    rect.bottom - rect.top
}

fn approx_eq(a: i32, b: i32, tolerance: i32) -> bool {
    (a - b).abs() <= tolerance
}

/// "Half"/"quarter" descriptions match Windows' own Snap layouts
/// (Win+Left/Right, Win+corner) since that's how most restored windows
/// end up at a round fraction of the screen. Anything else falls back
/// to an approximate, rounded percentage rather than exact coordinates.
fn describe_portion(window: &RECT, monitor: &RECT) -> (bool, Option<String>) {
    let monitor_w = rect_width(monitor).max(1);
    let monitor_h = rect_height(monitor).max(1);
    let window_w = rect_width(window).max(0);
    let window_h = rect_height(window).max(0);

    let area_ratio = (window_w as f64 * window_h as f64) / (monitor_w as f64 * monitor_h as f64);

    if area_ratio >= 0.9 {
        return (true, None);
    }

    let half_w = monitor_w / 2;
    let half_h = monitor_h / 2;
    let is_left = approx_eq(window.left, monitor.left, EDGE_TOLERANCE_PX);
    let is_right = approx_eq(window.right, monitor.right, EDGE_TOLERANCE_PX);
    let is_top = approx_eq(window.top, monitor.top, EDGE_TOLERANCE_PX);
    let is_bottom = approx_eq(window.bottom, monitor.bottom, EDGE_TOLERANCE_PX);
    let full_height = approx_eq(window_h, monitor_h, EDGE_TOLERANCE_PX * 2);
    let full_width = approx_eq(window_w, monitor_w, EDGE_TOLERANCE_PX * 2);
    let half_width_match = approx_eq(window_w, half_w, EDGE_TOLERANCE_PX * 2);
    let half_height_match = approx_eq(window_h, half_h, EDGE_TOLERANCE_PX * 2);

    if half_width_match && full_height {
        if is_left {
            return (false, Some("the left half".to_string()));
        }
        if is_right {
            return (false, Some("the right half".to_string()));
        }
    }
    if half_height_match && full_width {
        if is_top {
            return (false, Some("the top half".to_string()));
        }
        if is_bottom {
            return (false, Some("the bottom half".to_string()));
        }
    }
    if half_width_match && half_height_match {
        let vertical = if is_top { "top" } else if is_bottom { "bottom" } else { "" };
        let horizontal = if is_left { "left" } else if is_right { "right" } else { "" };
        if !vertical.is_empty() && !horizontal.is_empty() {
            return (false, Some(format!("the {vertical}-{horizontal} quarter")));
        }
    }

    let rounded_tenth = ((area_ratio * 10.0).round() as i32).clamp(1, 9) * 10;
    (false, Some(format!("roughly {rounded_tenth} percent")))
}

#[tauri::command]
pub fn get_capture_context() -> Result<CaptureContext, String> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return Err("No active window was found.".to_string());
        }

        let app_name = process_app_name(hwnd);
        let window_title = window_title(hwnd);
        let show_state = window_show_state(hwnd);

        if show_state == SW_SHOWMINIMIZED {
            return Ok(CaptureContext {
                app_name,
                window_title,
                state: "minimized".to_string(),
                monitor_number: None,
                monitor_width: None,
                monitor_height: None,
                window_width: None,
                window_height: None,
                fills_screen: None,
                portion: None,
                extends_beyond_monitor: None,
                capture_target: "monitor".to_string(),
            });
        }

        let mut window_rect = RECT::default();
        let _ = GetWindowRect(hwnd, &mut window_rect);

        let hmonitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let monitor_match = locate_monitor(hmonitor);

        let (monitor_number, monitor_rect) = match &monitor_match {
            Some(m) => (Some(m.number), Some(m.rect)),
            None => (None, None),
        };

        let is_fullscreen_geometry = monitor_rect
            .map(|m| {
                approx_eq(window_rect.left, m.left, EDGE_TOLERANCE_PX)
                    && approx_eq(window_rect.top, m.top, EDGE_TOLERANCE_PX)
                    && approx_eq(window_rect.right, m.right, EDGE_TOLERANCE_PX)
                    && approx_eq(window_rect.bottom, m.bottom, EDGE_TOLERANCE_PX)
            })
            .unwrap_or(false);

        let state = if is_fullscreen_geometry {
            "fullscreen".to_string()
        } else if show_state == SW_SHOWMAXIMIZED {
            "maximized".to_string()
        } else {
            "restored".to_string()
        };

        let (fills_screen, portion) = match &monitor_rect {
            Some(m) => {
                let (fills, portion) = describe_portion(&window_rect, m);
                (Some(fills || is_fullscreen_geometry), portion)
            }
            None => (None, None),
        };

        let extends_beyond_monitor = monitor_rect.map(|m| {
            window_rect.left < m.left - EDGE_TOLERANCE_PX
                || window_rect.top < m.top - EDGE_TOLERANCE_PX
                || window_rect.right > m.right + EDGE_TOLERANCE_PX
                || window_rect.bottom > m.bottom + EDGE_TOLERANCE_PX
        });

        Ok(CaptureContext {
            app_name,
            window_title,
            state,
            monitor_number,
            monitor_width: monitor_rect.as_ref().map(rect_width),
            monitor_height: monitor_rect.as_ref().map(rect_height),
            window_width: Some(rect_width(&window_rect)),
            window_height: Some(rect_height(&window_rect)),
            fills_screen,
            portion,
            extends_beyond_monitor,
            capture_target: "monitor".to_string(),
        })
    }
}

/// A comparable key covering only the fields that count as a
/// "meaningful" change for the Capture Context Descriptor (see
/// docs/Screen Reader First Principles.md) - not window size/position
/// in raw form, since those change constantly during a drag/resize in
/// ways that aren't worth a fresh announcement.
pub fn context_key(context: &CaptureContext) -> String {
    format!(
        "{}|{}|{}|{:?}|{:?}|{:?}",
        context.app_name,
        context.window_title,
        context.state,
        context.monitor_number,
        context.fills_screen,
        context.portion,
    )
}
