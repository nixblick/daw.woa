//! Ablagepunkt für Dateien, die aus dem Explorer aufs Fenster gezogen werden (#593).
//!
//! Beim Ziehen aus dem Windows-Explorer kommen keine Mausbewegungen an, und winit reicht den
//! Ablagepunkt nicht weiter: `pointer.latest_pos()` ist dann leer. Unter Windows fragt WOA die
//! Cursorposition deshalb selbst ab (ohne neue Abhängigkeit, wie `apps/woa/src/power.rs`).

use eframe::egui;

/// Rechnet Pixel im Client-Bereich des Fensters in egui-Punkte um.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn client_px_to_points(x: i32, y: i32, pixels_per_point: f32) -> egui::Pos2 {
    let ppp = if pixels_per_point > 0.0 {
        pixels_per_point
    } else {
        1.0
    };
    egui::pos2(x as f32 / ppp, y as f32 / ppp)
}

/// Cursor des Betriebssystems in egui-Punkten, wenn er über einem WOA-Fenster steht.
#[cfg(windows)]
fn os_cursor(pixels_per_point: f32) -> Option<egui::Pos2> {
    use std::ffi::c_void;

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct Point {
        x: i32,
        y: i32,
    }
    const GA_ROOT: u32 = 2;

    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetCursorPos(p: *mut Point) -> i32;
        fn WindowFromPoint(p: Point) -> *mut c_void;
        fn GetAncestor(hwnd: *mut c_void, flags: u32) -> *mut c_void;
        fn GetWindowThreadProcessId(hwnd: *mut c_void, pid: *mut u32) -> u32;
        fn ScreenToClient(hwnd: *mut c_void, p: *mut Point) -> i32;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcessId() -> u32;
    }

    let mut p = Point::default();
    // SAFETY: Zeiger auf lokale Strukturen im von Windows erwarteten Layout (zwei i32); die
    // Fenster-Handles stammen direkt aus Windows und werden nur an Windows zurückgegeben.
    unsafe {
        if GetCursorPos(&raw mut p) == 0 {
            return None;
        }
        let under = WindowFromPoint(p);
        if under.is_null() {
            return None;
        }
        let root = GetAncestor(under, GA_ROOT);
        let hwnd = if root.is_null() { under } else { root };
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &raw mut pid);
        if pid != GetCurrentProcessId() {
            return None;
        }
        if ScreenToClient(hwnd, &raw mut p) == 0 {
            return None;
        }
    }
    Some(client_px_to_points(p.x, p.y, pixels_per_point))
}

#[cfg(not(windows))]
fn os_cursor(_pixels_per_point: f32) -> Option<egui::Pos2> {
    None
}

/// Wo wurde abgelegt? Erst die Zeigerposition von egui, sonst (Explorer-Drop unter Windows)
/// der Cursor des Betriebssystems. `None`: Ort unbekannt, der Aufrufer zeigt einen Hinweis.
pub fn drop_position(ctx: &egui::Context) -> Option<egui::Pos2> {
    ctx.input(|i| i.pointer.latest_pos())
        .or_else(|| os_cursor(ctx.pixels_per_point()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixels_become_points() {
        assert_eq!(client_px_to_points(300, 150, 1.5), egui::pos2(200.0, 100.0));
        assert_eq!(client_px_to_points(10, 20, 0.0), egui::pos2(10.0, 20.0));
    }
}
