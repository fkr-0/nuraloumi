//! Pure motion primitives with no timer or runtime dependency.

/// Standard duration classes in milliseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionClass {
    Fast,
    Normal,
    Slow,
}

impl MotionClass {
    pub const fn duration_ms(self) -> u32 {
        match self {
            Self::Fast => 120,
            Self::Normal => 180,
            Self::Slow => 240,
        }
    }
}

/// Motion accessibility mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionMode {
    Full,
    Reduced,
}

/// Renderer-neutral finite transition.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transition {
    pub opacity: f32,
    pub translate_y: f32,
    pub scale: f32,
}

const ENTER_TRANSLATE_Y: f32 = 8.0;

/// Convert elapsed milliseconds to bounded normalized progress.
pub fn normalized_progress(elapsed_ms: u32, class: MotionClass) -> f32 {
    (elapsed_ms as f32 / class.duration_ms() as f32).clamp(0.0, 1.0)
}

pub fn ease_out_cubic(progress: f32) -> f32 {
    let inverse = 1.0 - progress.clamp(0.0, 1.0);
    1.0 - inverse * inverse * inverse
}

pub fn ease_in_cubic(progress: f32) -> f32 {
    let progress = progress.clamp(0.0, 1.0);
    progress * progress * progress
}

pub fn ease_in_out_cubic(progress: f32) -> f32 {
    let progress = progress.clamp(0.0, 1.0);
    if progress < 0.5 {
        4.0 * progress * progress * progress
    } else {
        1.0 - (-2.0 * progress + 2.0).powi(3) / 2.0
    }
}

/// Enter with opacity and at most 8 logical pixels of translation.
pub fn enter_transition(progress: f32, mode: MotionMode) -> Transition {
    if mode == MotionMode::Reduced {
        return reduced_motion(progress > 0.0);
    }
    let progress = progress.clamp(0.0, 1.0);
    Transition {
        opacity: progress,
        translate_y: (1.0 - progress) * ENTER_TRANSLATE_Y,
        scale: 1.0,
    }
}

/// Exit with opacity and at most 8 logical pixels of translation.
pub fn exit_transition(progress: f32, mode: MotionMode) -> Transition {
    if mode == MotionMode::Reduced {
        return reduced_motion(progress < 1.0);
    }
    let progress = progress.clamp(0.0, 1.0);
    Transition {
        opacity: 1.0 - progress,
        translate_y: progress * ENTER_TRANSLATE_Y,
        scale: 1.0,
    }
}

pub fn fade_in(progress: f32) -> f32 {
    progress.clamp(0.0, 1.0)
}

pub fn fade_out(progress: f32) -> f32 {
    1.0 - progress.clamp(0.0, 1.0)
}

/// Reduced motion has immediate visibility and no spatial movement or scaling.
pub const fn reduced_motion(visible: bool) -> Transition {
    Transition {
        opacity: if visible { 1.0 } else { 0.0 },
        translate_y: 0.0,
        scale: 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_classes_match_contract() {
        assert_eq!(MotionClass::Fast.duration_ms(), 120);
        assert_eq!(MotionClass::Normal.duration_ms(), 180);
        assert_eq!(MotionClass::Slow.duration_ms(), 240);
    }

    #[test]
    fn normalized_progress_is_bounded() {
        assert_eq!(normalized_progress(0, MotionClass::Normal), 0.0);
        assert_eq!(normalized_progress(90, MotionClass::Normal), 0.5);
        assert_eq!(normalized_progress(1_000, MotionClass::Normal), 1.0);
    }

    #[test]
    fn transition_outputs_are_bounded_and_settle() {
        for progress in [-1.0, 0.0, 0.5, 1.0, 2.0] {
            for transition in [
                enter_transition(progress, MotionMode::Full),
                exit_transition(progress, MotionMode::Full),
            ] {
                assert!((0.0..=1.0).contains(&transition.opacity));
                assert!((0.0..=ENTER_TRANSLATE_Y).contains(&transition.translate_y));
                assert_eq!(transition.scale, 1.0);
            }
            assert!((0.0..=1.0).contains(&fade_in(progress)));
            assert!((0.0..=1.0).contains(&fade_out(progress)));
        }

        assert_eq!(enter_transition(1.0, MotionMode::Full).translate_y, 0.0);
        assert_eq!(exit_transition(1.0, MotionMode::Full).opacity, 0.0);
    }

    #[test]
    fn easing_curves_keep_endpoints_and_bounds() {
        for easing in [ease_out_cubic, ease_in_cubic, ease_in_out_cubic] {
            assert_eq!(easing(0.0), 0.0);
            assert_eq!(easing(1.0), 1.0);
            for progress in [-1.0, 0.25, 0.75, 2.0] {
                assert!((0.0..=1.0).contains(&easing(progress)));
            }
        }
    }

    #[test]
    fn reduced_motion_removes_spatial_changes() {
        assert_eq!(
            enter_transition(0.5, MotionMode::Reduced),
            Transition {
                opacity: 1.0,
                translate_y: 0.0,
                scale: 1.0,
            }
        );
        assert_eq!(exit_transition(1.0, MotionMode::Reduced).opacity, 0.0);
    }
}
