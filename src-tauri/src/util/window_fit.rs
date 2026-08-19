//! Keep the main window inside the monitor work area (visible screen minus taskbar).
//!
//! On Windows, creating a decorated window and then stripping chrome (`SWP_NOSIZE`)
//! leaves the HWND as tall as inner size + native titlebar, so the bottom edge
//! can sit below the work area. This module restores the configured inner size
//! and clamps outer bounds to the work area.

/// Matches `src-tauri/tauri.conf.json` → `app.windows[0]`.
pub const DEFAULT_INNER_WIDTH: f64 = 1280.0;
pub const DEFAULT_INNER_HEIGHT: f64 = 860.0;
pub const MIN_INNER_WIDTH: f64 = 960.0;
pub const MIN_INNER_HEIGHT: f64 = 640.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowFit {
    pub inner_width: u32,
    pub inner_height: u32,
    pub outer_x: i32,
    pub outer_y: i32,
}

impl WindowFit {
    pub fn size_changed(&self, inner: (u32, u32)) -> bool {
        self.inner_width != inner.0 || self.inner_height != inner.1
    }

    pub fn pos_changed(&self, outer_pos: (i32, i32)) -> bool {
        self.outer_x != outer_pos.0 || self.outer_y != outer_pos.1
    }
}

/// Shrink an inflated inner size back to the configured default, then fit the
/// **outer** rect into `work` (physical pixels).
pub fn compute_window_fit(
    inner: (u32, u32),
    outer: (u32, u32),
    outer_pos: (i32, i32),
    work_pos: (i32, i32),
    work_size: (u32, u32),
    preferred_inner: (u32, u32),
) -> WindowFit {
    let chrome_w = outer.0.saturating_sub(inner.0);
    let chrome_h = outer.1.saturating_sub(inner.1);

    let max_inner_w = work_size.0.saturating_sub(chrome_w).max(1);
    let max_inner_h = work_size.1.saturating_sub(chrome_h).max(1);

    let inner_width = inner.0.min(preferred_inner.0).min(max_inner_w).max(1);
    let inner_height = inner.1.min(preferred_inner.1).min(max_inner_h).max(1);

    let outer_w = inner_width.saturating_add(chrome_w);
    let outer_h = inner_height.saturating_add(chrome_h);

    let max_x = work_pos.0 + work_size.0 as i32 - outer_w as i32;
    let max_y = work_pos.1 + work_size.1 as i32 - outer_h as i32;

    WindowFit {
        inner_width,
        inner_height,
        outer_x: outer_pos.0.clamp(work_pos.0, max_x.max(work_pos.0)),
        outer_y: outer_pos.1.clamp(work_pos.1, max_y.max(work_pos.1)),
    }
}

pub fn preferred_inner_physical(scale_factor: f64) -> (u32, u32) {
    let scale = if scale_factor.is_finite() && scale_factor > 0.0 {
        scale_factor
    } else {
        1.0
    };
    (
        (DEFAULT_INNER_WIDTH * scale).round() as u32,
        (DEFAULT_INNER_HEIGHT * scale).round() as u32,
    )
}

pub fn min_inner_physical(scale_factor: f64) -> (u32, u32) {
    let scale = if scale_factor.is_finite() && scale_factor > 0.0 {
        scale_factor
    } else {
        1.0
    };
    (
        (MIN_INNER_WIDTH * scale).round() as u32,
        (MIN_INNER_HEIGHT * scale).round() as u32,
    )
}

/// Apply fit to the main window. Best-effort; never fails startup.
pub fn fit_main_window(window: &tauri::WebviewWindow) {
    let Ok(inner) = window.inner_size() else {
        return;
    };
    let Ok(outer) = window.outer_size() else {
        return;
    };
    let Ok(pos) = window.outer_position() else {
        return;
    };
    let monitor = match window.current_monitor() {
        Ok(Some(m)) => m,
        _ => match window.primary_monitor() {
            Ok(Some(m)) => m,
            _ => return,
        },
    };
    let work = monitor.work_area();
    let scale = window.scale_factor().unwrap_or(monitor.scale_factor());
    let preferred = preferred_inner_physical(scale);
    let fit = compute_window_fit(
        (inner.width, inner.height),
        (outer.width, outer.height),
        (pos.x, pos.y),
        (work.position.x, work.position.y),
        (work.size.width, work.size.height),
        preferred,
    );

    if fit.size_changed((inner.width, inner.height)) {
        let min = min_inner_physical(scale);
        // Work area can be shorter than minHeight (scaled 1080p). Prefer fitting.
        let min_w = min.0.min(fit.inner_width).max(1);
        let min_h = min.1.min(fit.inner_height).max(1);
        let _ = window.set_min_size(Some(tauri::Size::Physical(tauri::PhysicalSize::new(
            min_w, min_h,
        ))));
        if let Err(e) = window.set_size(tauri::Size::Physical(tauri::PhysicalSize::new(
            fit.inner_width,
            fit.inner_height,
        ))) {
            eprintln!("fit_main_window set_size failed: {e}");
        }
        let _ = window.set_min_size(Some(tauri::Size::Logical(tauri::LogicalSize::new(
            MIN_INNER_WIDTH,
            MIN_INNER_HEIGHT.min(fit.inner_height as f64 / scale.max(0.01)),
        ))));
    }

    if fit.pos_changed((pos.x, pos.y)) {
        if let Err(e) = window.set_position(tauri::Position::Physical(tauri::PhysicalPosition::new(
            fit.outer_x,
            fit.outer_y,
        ))) {
            eprintln!("fit_main_window set_position failed: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn already_fits_is_unchanged() {
        let fit = compute_window_fit(
            (1280, 860),
            (1280, 860),
            (40, 40),
            (0, 0),
            (1920, 1040),
            (1280, 860),
        );
        assert_eq!(
            fit,
            WindowFit {
                inner_width: 1280,
                inner_height: 860,
                outer_x: 40,
                outer_y: 40,
            }
        );
    }

    #[test]
    fn strips_titlebar_inflation_when_work_area_allows() {
        // Decorations toggle left inner 40px taller than the configured 860.
        let fit = compute_window_fit(
            (1280, 900),
            (1294, 916),
            (10, 10),
            (0, 0),
            (1920, 1040),
            (1280, 860),
        );
        assert_eq!(fit.inner_height, 860);
        assert_eq!(fit.inner_width, 1280);
        assert_eq!(fit.outer_y, 10);
    }

    #[test]
    fn clamps_height_to_work_area_and_moves_up() {
        // 1080p @ 125%: logical work ~816, window hung off the bottom.
        let fit = compute_window_fit(
            (1280, 900),
            (1280, 916),
            (0, 80),
            (0, 0),
            (1536, 816),
            (1280, 860),
        );
        assert_eq!(fit.inner_height, 800); // 816 work − 16 chrome
        assert_eq!(fit.outer_y, 0);
        assert_eq!(fit.inner_height + 16, 816);
    }

    #[test]
    fn bottom_overflow_repositions_without_shrink_when_height_fits() {
        let fit = compute_window_fit(
            (1280, 860),
            (1280, 860),
            (100, 400),
            (0, 0),
            (1920, 1040),
            (1280, 860),
        );
        assert_eq!(fit.inner_height, 860);
        assert_eq!(fit.outer_y, 180); // 1040 − 860
    }

    #[test]
    fn preferred_inner_scales() {
        assert_eq!(preferred_inner_physical(1.0), (1280, 860));
        assert_eq!(preferred_inner_physical(1.25), (1600, 1075));
    }
}
