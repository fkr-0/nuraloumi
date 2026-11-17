#!/usr/bin/env python3
"""Source-grounded, deterministic QoL press-feedback release gates.

Run: python3 -m unittest discover -s scripts -p test_qol_pressed_feedback.py -v
Tests intentionally fail red until R1-16/R1-17 provide executable contracts.
No compositor, network, timing sleeps, or device access.
"""
from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[1]
CORE = ROOT / "crates/nuraloumi-core/src/motion.rs"
PANEL = ROOT / "crates/nuraloumi-shell/src/bin/panel.rs"
LAYOUT = ROOT / "crates/nuraloumi-render-cairo/src/layout.rs"
MODEL = ROOT / "crates/nuraloumi-render-cairo/src/model.rs"


def source(path):
    return path.read_text(encoding="utf-8")


def function_body(text, name):
    """Extract balanced Rust function body, ignoring braces in strings/comments only imperfectly.

    The known target functions contain no problematic literal braces in their
    bodies; missing function is an actionable red gate.
    """
    match = re.search(r"\bfn\s+" + re.escape(name) + r"\s*\(", text)
    if not match:
        return ""
    start = text.find("{", match.end())
    if start < 0:
        return ""
    depth = 0
    for i in range(start, len(text)):
        if text[i] == "{":
            depth += 1
        elif text[i] == "}":
            depth -= 1
            if depth == 0:
                return text[start:i + 1]
    return ""


class ExistingBaseline(unittest.TestCase):
    def test_fast_class_is_120_ms(self):
        self.assertRegex(source(CORE), r"Self::Fast\s*=>\s*120")

    def test_existing_menu_paint_uses_pressed_id(self):
        self.assertIn("interaction.pressed_id", source(LAYOUT))
        self.assertIn("pressed_id: Option<String>", source(MODEL))

    def test_multi_touch_cancellation_retained(self):
        body = function_body(source(PANEL), "begin_panel_touch")
        self.assertIn("*pressed = None", body)

    def test_physical_minimum_touch_width_already_exercised(self):
        self.assertIn("hit.rect.width >= 48.0", source(PANEL))

    def test_existing_synthetic_pointer_guard(self):
        self.assertIn("suppress_pointer", source(PANEL))
        self.assertIn("touch_then_synthetic_pointer_is_one_activation", source(PANEL))


class PressedReleaseGates(unittest.TestCase):
    def test_panel_down_marks_visual_dirty_and_presents(self):
        panel = source(PANEL)
        # The event arm must change the visual scene immediately, not merely
        # remember the target for pointer-up activation.
        # Only the primary (BTN_LEFT) event should arm the feedback. The
        # panel event match exits early and must commit before 'continue'.
        arm = panel.split("button: 0x110,", 1)
        self.assertEqual(len(arm), 2, "primary panel pointer arm moved")
        down = arm[1].split("WaylandEvent::TouchDown { id, x, y } => {", 1)[0]
        self.assertIn("panel_press_feedback", down)
        self.assertIn("panel_press_dirty = true", down)
        panel_input = panel.split("if event.surface == Some(panel_surface) {", 1)[1]
        panel_input = panel_input.split("let dismiss_surface", 1)[0]
        self.assertIn("if panel_press_dirty {", panel_input)
        self.assertIn("render_panel_feedback_and_present(", panel_input)
        self.assertLess(
            panel_input.rfind("render_panel_feedback_and_present("),
            panel_input.rfind("continue;"),
            "pressed pixels must be presented before the panel event bypass",
        )

    def test_panel_touch_down_and_cancel_invalidate_visual(self):
        panel = source(PANEL)
        down = panel.split("WaylandEvent::TouchDown { id, x, y } => {", 1)
        self.assertEqual(len(down), 2)
        block = down[1].split("WaylandEvent::TouchMotion", 1)[0]
        self.assertRegex(block, r"(?:render_panel_and_present|request_redraw|panel_visual|press_feedback|pressed_visual|dirty_panel)")
        cancel = panel.split("WaylandEvent::TouchCancel { ids } => {", 1)
        self.assertEqual(len(cancel), 2)
        block = cancel[1].split("WaylandEvent::Close", 1)[0]
        self.assertRegex(block, r"(?:render_panel_and_present|request_redraw|panel_visual|press_feedback|pressed_visual|dirty_panel)")

    def test_panel_scene_accepts_ephemeral_press_state(self):
        panel = source(PANEL)
        body = function_body(panel, "render_panel_feedback_and_present")
        overlay = function_body(panel, "panel_scene_with_press")
        self.assertIn("panel_scene_with_press(", body)
        self.assertIn("press_feedback.sample(", body)
        self.assertIn("present_scene(", body)
        self.assertIn("hit.actionable && hit.enabled", overlay)
        self.assertIn("scene.paint.insert(", overlay)
        self.assertIn("battery", overlay)
        self.assertIn("clock", overlay)

    def test_finite_release_and_reduced_motion_primitives(self):
        core = source(CORE)
        self.assertTrue(re.search(r"(?:PressTransition|PressedTransition|PressFeedback|pressed_transition|press_transition)", core), "generic press transition missing")
        self.assertRegex(core, r"(?:MotionMode::Reduced|reduced_motion)")
        self.assertRegex(core, r"(?:normalized_progress|duration_ms)")

    def test_keyboard_enter_space_press_parity(self):
        panel = source(PANEL)
        self.assertTrue(re.search(r"(?:Key::Space|Key::Enter|Key::Return|Keysym::space|Keysym::Return)", panel), "no explicit Enter/Space press parity")
        self.assertTrue(re.search(r"(?:pressed_visual|press_feedback|press_state|held_pressed|panel_pressed)", panel), "keyboard path not connected to pressed feedback")

    def test_no_status_only_press_highlight(self):
        panel = source(PANEL)
        scene = function_body(panel, "build_panel_scene")
        self.assertTrue(scene)
        self.assertRegex(scene, r"(?:actionable|interactive|enabled|hit|affordance)")
        # This gate is a static prerequisite, not a proof that every icon is actionable.
        self.assertRegex(source(LAYOUT), r"item\.kind\.actionable\(\)")

    def test_viewport_and_scale_source_coverage(self):
        panel = source(PANEL)
        self.assertIn("Viewport::new(", panel)
        self.assertIn("scale.max(1)", panel)
        self.assertIn("hit.rect.width >= 48.0", panel)

    def test_no_idle_animation_polling_added(self):
        # Static detector for obvious unconditional redraw/sleep loops only;
        # bounded event-loop scheduling still needs code review.
        panel = source(PANEL)
        self.assertNotRegex(panel, r"loop\s*\{\s*(?:std::thread::sleep|thread::sleep)")
        self.assertNotRegex(panel, r"loop\s*\{\s*render_panel_and_present")


if __name__ == "__main__":
    unittest.main()
