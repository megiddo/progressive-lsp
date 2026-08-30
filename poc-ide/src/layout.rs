//! `LayoutState` value object: resizable left-panel width, no window handle.

/// In-process left-panel width. `left_width` is always &gt; 0 after clamp.
#[derive(Clone, Debug, PartialEq)]
pub struct LayoutState {
    left_width: f32,
}

impl LayoutState {
    pub const MIN_LEFT_WIDTH: f32 = 80.0;
    pub const MAX_LEFT_WIDTH: f32 = 640.0;
    pub const DEFAULT_LEFT_WIDTH: f32 = 220.0;

    pub fn new() -> Self {
        Self {
            left_width: Self::DEFAULT_LEFT_WIDTH,
        }
    }

    pub fn with_left_width(width: f32) -> Self {
        Self {
            left_width: clamp_left_width(width),
        }
    }

    pub fn set_left_width(&mut self, width: f32) {
        self.left_width = clamp_left_width(width);
    }

    pub fn left_width(&self) -> f32 {
        self.left_width
    }
}

impl Default for LayoutState {
    fn default() -> Self {
        Self::new()
    }
}

fn clamp_left_width(width: f32) -> f32 {
    if !width.is_finite() {
        return LayoutState::DEFAULT_LEFT_WIDTH;
    }
    if width < LayoutState::MIN_LEFT_WIDTH {
        LayoutState::MIN_LEFT_WIDTH
    } else if width > LayoutState::MAX_LEFT_WIDTH {
        LayoutState::MAX_LEFT_WIDTH
    } else {
        width
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_state_value_object_default_is_positive() {
        let layout = LayoutState::new();
        assert_eq!(layout.left_width(), LayoutState::DEFAULT_LEFT_WIDTH);
        assert!(layout.left_width() > 0.0);
        assert_eq!(LayoutState::default(), layout);
        assert_eq!(LayoutState::DEFAULT_LEFT_WIDTH, 220.0);
        assert_eq!(LayoutState::MIN_LEFT_WIDTH, 80.0);
        assert_eq!(LayoutState::MAX_LEFT_WIDTH, 640.0);
    }

    #[test]
    fn layout_state_value_object_clamps_on_set_and_construct() {
        let mut layout = LayoutState::with_left_width(0.0);
        assert_eq!(layout.left_width(), LayoutState::MIN_LEFT_WIDTH);
        layout.set_left_width(-12.0);
        assert_eq!(layout.left_width(), LayoutState::MIN_LEFT_WIDTH);
        layout.set_left_width(79.9);
        assert_eq!(layout.left_width(), LayoutState::MIN_LEFT_WIDTH);
        layout.set_left_width(80.0);
        assert_eq!(layout.left_width(), 80.0);
        layout.set_left_width(120.5);
        assert_eq!(layout.left_width(), 120.5);
        layout.set_left_width(640.0);
        assert_eq!(layout.left_width(), 640.0);
        layout.set_left_width(641.0);
        assert_eq!(layout.left_width(), LayoutState::MAX_LEFT_WIDTH);
        layout.set_left_width(10_000.0);
        assert_eq!(layout.left_width(), LayoutState::MAX_LEFT_WIDTH);
    }

    #[test]
    fn layout_state_value_object_non_finite_uses_default() {
        let nan = LayoutState::with_left_width(f32::NAN);
        assert_eq!(nan.left_width(), LayoutState::DEFAULT_LEFT_WIDTH);
        let inf = LayoutState::with_left_width(f32::INFINITY);
        assert_eq!(inf.left_width(), LayoutState::DEFAULT_LEFT_WIDTH);
        let ninf = LayoutState::with_left_width(f32::NEG_INFINITY);
        assert_eq!(ninf.left_width(), LayoutState::DEFAULT_LEFT_WIDTH);
        assert!(LayoutState::with_left_width(f32::NAN).left_width() > 0.0);
    }

    #[test]
    fn layout_state_value_object_has_no_window_handle_in_debug() {
        let debug = format!("{:?}", LayoutState::new());
        assert!(debug.contains("left_width"));
        assert!(!debug.to_lowercase().contains("window"));
        assert!(!debug.to_lowercase().contains("egui"));
    }
}
