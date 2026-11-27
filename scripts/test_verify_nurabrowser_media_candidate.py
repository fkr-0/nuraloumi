import tempfile
import unittest
from pathlib import Path
from scripts.verify_nurabrowser_media_candidate import evaluate


class MediaPreflightNegatives(unittest.TestCase):
    def test_disabled_features_and_missing_plugins(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            result = evaluate(root, root / "missing-cache")
            self.assertEqual(result["MEDIA_CANDIDATE_READY"], "no")
            self.assertIn("feature-disabled-or-cache-missing:ENABLE_VIDEO", result["failures"])
            self.assertIn("missing-plugin:libgstplayback.so", result["failures"])

    def test_on_features_cannot_hide_missing_codec_or_abi(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            cache = root / "CMakeCache.txt"
            cache.write_text("ENABLE_VIDEO:BOOL=ON\nENABLE_WEB_AUDIO:BOOL=ON\nUSE_GSTREAMER:BOOL=ON\n")
            result = evaluate(root, cache)
            self.assertIn("missing-wpe-elf", result["failures"])
            self.assertIn("missing-libav-h264-aac-decoder-candidate", result["failures"])


if __name__ == "__main__":
    unittest.main()
