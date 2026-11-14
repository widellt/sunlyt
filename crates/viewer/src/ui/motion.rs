//! The viewer's only easing curves and durations (viewer-design skill).
//! Chrome only: simulated time and data are never eased.

use eframe::egui;

/// Panels and popovers, s.
pub const PANEL_S: f32 = 0.2;
/// Exits run faster than entries, s.
pub const EXIT_S: f32 = 0.12;
/// Delay before the first tooltip, s.
pub const TOOLTIP_DELAY_S: f32 = 0.4;
/// Camera preset glide, s.
pub const GLIDE_S: f32 = 0.22;

/// Cubic Bézier `(0,0) (x1,y1) (x2,y2) (1,1)` evaluated at `x`, as CSS
/// `cubic-bezier()`: solve x(s) = x for the curve parameter, return y(s).
fn bezier(x1: f32, y1: f32, x2: f32, y2: f32, x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    let coord = |a: f32, b: f32, s: f32| {
        let m = 1.0 - s;
        3.0 * m * m * s * a + 3.0 * m * s * s * b + s * s * s
    };
    // Bisection: x(s) is monotone for 0 ≤ x1, x2 ≤ 1. 24 halvings give
    // ~6e-8, far below a pixel.
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..24 {
        let mid = 0.5 * (lo + hi);
        if coord(x1, x2, mid) < x {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    coord(y1, y2, 0.5 * (lo + hi))
}

/// Entering or exiting: strong ease-out, `cubic-bezier(0.23, 1, 0.32, 1)`.
pub fn ease_out(x: f32) -> f32 {
    bezier(0.23, 1.0, 0.32, 1.0, x)
}

/// Moving on screen: `cubic-bezier(0.77, 0, 0.175, 1)`.
pub fn ease_in_out(x: f32) -> f32 {
    bezier(0.77, 0.0, 0.175, 1.0, x)
}

/// Opacity 0…1 of something appearing (`on`) or leaving: ease-out, with
/// the exit faster than the entry. Kept under reduced motion (fades only).
pub fn fade(ctx: &egui::Context, id: egui::Id, on: bool) -> f32 {
    let secs = if on { PANEL_S } else { EXIT_S };
    ctx.animate_bool_with_time_and_easing(id, on, secs, ease_out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curves_hit_their_ends_and_are_monotone() {
        for f in [ease_out, ease_in_out] {
            assert!(f(0.0).abs() < 1e-5 && (f(1.0) - 1.0).abs() < 1e-5);
            let ys: Vec<f32> = (0..=100).map(|i| f(i as f32 / 100.0)).collect();
            assert!(ys.windows(2).all(|w| w[1] >= w[0] - 1e-6));
        }
        // Strong ease-out: most of the motion happens early.
        assert!(ease_out(0.25) > 0.75);
        // In-out: slow at both ends (the curve is not quite symmetric).
        assert!(ease_in_out(0.2) < 0.2 && ease_in_out(0.8) > 0.8);
    }
}
