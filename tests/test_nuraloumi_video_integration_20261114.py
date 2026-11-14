"""The one NuraLoumi player/daemon + private four-pass renderer contract.

Tests are host-only and never modify the running SL101 or installed Mesa.
"""
import hashlib
from pathlib import Path
import re
import subprocess
import unittest

ROOT = Path(__file__).resolve().parents[1]
ENGINE = ROOT / "components/nura-media-engine"
SHELL = ROOT / "crates/nuraloumi-shell/src"
TARGET = ROOT / "target"


class OnePlayerTests(unittest.TestCase):
    def test_exactly_one_ui_one_service_and_no_extra_player_executable(self):
        cargo = (ROOT / "crates/nuraloumi-shell/Cargo.toml").read_text()
        self.assertEqual(cargo.count('name = "nuraloumi-player"'), 1)
        self.assertEqual(cargo.count('name = "nuraloumi-playerd"'), 1)
        self.assertNotIn('name = "nuraloumi-video-player"', cargo)
        self.assertNotIn("int main(int argc", (ENGINE / "src/vde-moving-probe.c").read_text())
        self.assertIn("int nura_video_render_loop(", (ENGINE / "src/vde-moving-probe.c").read_text())
        self.assertIn("pthread_create(", (ENGINE / "src/video-bridge.c").read_text())
        self.assertIn("pthread_join(", (ENGINE / "src/video-bridge.c").read_text())
        ipc = (SHELL / "bin/playerd.rs").read_text()
        self.assertIn('nuraloumi-player.sock', ipc)
        self.assertNotIn("nuraloumi-video.sock", ipc)
        self.assertIn("video: Option<VideoEngine>", (SHELL / "player_engine.rs").read_text())

    def test_owner_ipc_commands_go_to_in_process_bridge(self):
        native = (SHELL / "video_engine.rs").read_text()
        engine = (SHELL / "player_engine.rs").read_text()
        for abi in (
            "nura_video_open", "nura_video_command", "nura_video_status",
            "nura_video_close", "libnuraloumi-video.so",
        ):
            self.assertIn(abi, native)
        for command in ("video.play()?", "video.pause()?", "video.seek(position_ms)?",
                        "video.set_volume(level)?", "video.set_mute(muted)?",
                        "self.video.take()"):
            self.assertIn(command, engine)
        for state in ("VideoPoll::Eos", "VideoPoll::Paused", "VideoPoll::Playing"):
            self.assertIn(state, engine)
        self.assertIn("GST_PLAY_FLAG_AUDIO: c_int = 2", engine)
        self.assertNotIn("gst-launch-1.0", engine)

    def test_native_dma_buf_no_software_yuv_and_typed_file_path(self):
        source = (ENGINE / "src/vde-moving-probe.c").read_text()
        for item in ("gst_dmabuf_memory_get_fd", "eglCreateImageKHR",
                     "glEGLImageTargetTexture2DOES", "gst_app_sink_try_pull_sample",
                     "gbm_surface_lock_front_buffer", "sl101_stream_submit",
                     "gst_element_seek_simple", "gst_element_set_state(pipeline,GST_STATE_PAUSED)",
                     '"location",nura_video_path(handle)'):
            self.assertIn(item, source)
        self.assertNotIn("gst_video_frame_map(", source)
        self.assertNotIn("videoconvert", source)
        self.assertNotIn("gst-launch-1.0", source)
        self.assertNotIn("glTexSubImage2D", source)
        self.assertIn("STREAM_LAYOUT_FAIL", source)
        self.assertIn("PIPELINE_EOS", source)
        wl = (ENGINE / "src/wayland-stream.c").read_text()
        self.assertIn("gbm_surface_release_buffer(p->surface,p->bo)", wl)
        self.assertIn("WAYLAND_STREAM_RESULT", wl)

    def test_private_bundle_sha_guard(self):
        script = (ROOT / "scripts/build-nuraloumi-player-video.sh").read_text()
        for digest in (
            "3745960c85ab21ecc33c40496995a01f806d27293c59cce711b3e38c6eef5977",
            "79ee247faf6397829d908136ad16e04dc936437a1c71b6d49120e470ec452608",
        ):
            self.assertIn(digest, script)
        self.assertIn("--network none", script)
        lib = TARGET / "nuraloumi-video-armv7/lib/libnuraloumi-video.so"
        if not lib.is_file():
            self.skipTest("ARM native library build not present")
        result = subprocess.run(["readelf", "-h", str(lib)],
                                text=True, capture_output=True, check=True)
        self.assertIn("Machine:                           ARM", result.stdout)
        result = subprocess.run(["nm", "-D", str(lib)],
                                text=True, capture_output=True, check=True)
        for name in ("nura_video_open", "nura_video_command",
                     "nura_video_status", "nura_video_close"):
            self.assertIn(name, result.stdout)

    def test_hardware_full_eos_and_buffer_lifecycle(self):
        log = TARGET / "nuraloumi-player-integrated-e2e.log"
        if not log.is_file():
            self.skipTest("Device E2E evidence not available")
        trace = log.read_text(errors="replace")
        self.assertIn("PLAYERD_INTEGRATED_E2E_PASS video_frames=414", trace)
        self.assertIn("STREAM_RESULT requested=0 presented=414", trace)
        self.assertIn("wayland_callbacks=414", trace)
        self.assertIn("video_eos=1 pipeline_eos=1", trace)
        self.assertIn("stream_cleanup_ok=1 bus_errors=0 cpu_yuv_map=0", trace)
        self.assertIn(
            "WAYLAND_STREAM_RESULT submitted=414 callbacks=414 "
            "releases=414 outstanding=0 pass=1", trace)
        fps = re.search(r"fps_presented=([\d.]+)", trace)
        self.assertIsNotNone(fps)
        self.assertGreater(float(fps.group(1)), 23)

    def test_hardware_pause_seek_volume_stop_and_all_releases(self):
        log = TARGET / "nuraloumi-player-controls-physical.log"
        if not log.is_file():
            self.skipTest("Device control E2E evidence not available")
        data = log.read_text(errors="replace")
        for marker in (
            "VIDEO_CONTROL pause_stable_delta_ms=0",
            "VIDEO_CONTROL paused_seek_ms=5000",
            "PLAYERD_CONTROL_E2E_PASS pause=1 seek=1 volume=1 mute=1 resume=1 stop=1",
            "WAYLAND_STREAM_RESULT submitted=147 callbacks=147 releases=147 outstanding=0 pass=1",
        ):
            self.assertIn(marker, data)
        stop = re.search(r"VIDEO_CONTROL stop_reply_ms=([\d.]+)", data)
        self.assertIsNotNone(stop)
        self.assertLess(float(stop.group(1)), 600.0)

    def test_host_native_library_abi_and_failure_paths(self):
        so = TARGET / "nuraloumi-video-host/libnuraloumi-video.so"
        if not so.is_file():
            self.skipTest("Host shared library not present")
        # Invalid path is rejected *before* spawning a worker thread.
        import ctypes
        library = ctypes.CDLL(str(so))
        library.nura_video_open.argtypes = [ctypes.c_char_p, ctypes.c_double, ctypes.c_int]
        library.nura_video_open.restype = ctypes.c_void_p
        self.assertIsNone(library.nura_video_open(b"relative.mp4", 0.5, 0))
        self.assertIsNone(library.nura_video_open(b"/tmp/a.mp4", float("nan"), 0))
        self.assertIsNone(library.nura_video_open(b"/tmp/a.mp4", 1.1, 0))

    def test_single_desktop_entry_advertises_video_and_exact_exec(self):
        desktop = (ROOT / "packaging/applications/nuraloumi-player.desktop").read_text()
        self.assertIn("Exec=nuraloumi-player %f", desktop)
        self.assertIn("video/mp4", desktop)
        self.assertNotIn("nuraloumi-video-player", desktop)
        self.assertIn("nuraloumi-playerd", (ENGINE / "README.md").read_text())


if __name__ == "__main__":
    unittest.main()
