#!/usr/bin/env python3
"""Isolated, fail-closed NuraBrowser Wayland pixel qualification.

No real seat, KMS, compositor socket, or browser profile is ever reused.
Exit 77 means prerequisites missing; exit 1 means an attempted run did not
qualify, and exit 0 is reserved for a fully verified Gate 3C.
"""

from __future__ import annotations

import argparse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import importlib.util
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import tempfile
import threading
import time


REPO = Path(__file__).resolve().parent.parent
DEFAULT_BROWSER = REPO / "target/debug/nuraloumi-browser"
ORACLE_HTML = b"""<!doctype html><html><head><title>NuraBrowser pixel oracle</title>
<meta charset="utf-8"><style>
body{margin:0;background:#112846;color:white;font:22px sans-serif}
#oracle{position:absolute;left:48px;top:72px;width:180px;height:120px;background:#04f51d}
#focus{position:absolute;left:48px;top:230px;width:200px;height:56px;background:#475569}
#focus:focus{background:#fafa05}
</style></head><body><div id="oracle"></div>
<input id="focus" aria-label="focus oracle" autofocus value="keyboard-test">
</body></html>"""


class OracleHandler(BaseHTTPRequestHandler):
    def do_GET(self) -> None:
        if self.path != "/":
            self.send_error(404)
            return
        self.send_response(200)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.send_header("Content-Length", str(len(ORACLE_HTML)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(ORACLE_HTML)

    def log_message(self, _format: str, *_args: object) -> None:
        pass


def resolved_executable(value: str) -> str | None:
    if "/" in value:
        path = Path(value).expanduser().resolve()
        return str(path) if path.is_file() and os.access(path, os.X_OK) else None
    return shutil.which(value)


def prereqs(args: argparse.Namespace) -> dict[str, object]:
    compositor = args.compositor
    compositor_bin = None
    if compositor == "auto":
        compositor_bin = shutil.which("sway") or shutil.which("labwc")
    else:
        compositor_bin = shutil.which(compositor)
    browser = resolved_executable(args.browser_bin)
    engine = resolved_executable(args.engine_bin)
    grim = shutil.which("grim")
    wtype = shutil.which("wtype")
    missing = []
    if not compositor_bin:
        missing.append("sway-or-labwc" if compositor == "auto" else compositor)
    if not grim:
        missing.append("grim")
    if not wtype:
        missing.append("wtype")
    if not browser:
        missing.append("browser-bin")
    if not engine:
        missing.append("engine-bin")
    if importlib.util.find_spec("PIL") is None:
        missing.append("Pillow")
    return {
        "status": "SKIP" if missing else "READY",
        "missing": missing,
        "compositor": Path(compositor_bin).name if compositor_bin else None,
        "compositor_bin": compositor_bin,
        "browser_bin": browser,
        "engine_bin": engine,
        "grim_bin": grim,
        "wtype_bin": wtype,
    }


def wait_for_socket(runtime: Path, proc: subprocess.Popen[bytes], timeout: float) -> str | None:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if proc.poll() is not None:
            return None
        for path in runtime.glob("wayland-*"):
            if path.is_socket():
                return path.name
        time.sleep(0.1)
    return None


def process_tree_rss(pid: int) -> dict[str, object]:
    """Attribute WebProcess descendants, without scanning unrelated host PIDs."""
    pending = [pid]
    seen: set[int] = set()
    values: dict[str, int | None] = {}
    while pending:
        current = pending.pop()
        if current in seen:
            continue
        seen.add(current)
        values[str(current)] = process_rss_kib(current)
        try:
            children = Path(f"/proc/{current}/task/{current}/children").read_text()
            pending.extend(int(item) for item in children.split())
        except (FileNotFoundError, PermissionError, ValueError):
            pass
    return {"process_kib": values, "sum_observed_kib": sum(v or 0 for v in values.values())}


def owned_process(argv: list[str], env: dict[str, str], log: Path) -> subprocess.Popen[bytes]:
    output = log.open("wb")
    try:
        proc = subprocess.Popen(
            argv, env=env, stdin=subprocess.DEVNULL, stdout=output,
            stderr=subprocess.STDOUT, start_new_session=True,
        )
    finally:
        output.close()
    return proc


def stop_owned(proc: subprocess.Popen[bytes] | None) -> None:
    if proc is None or proc.poll() is not None:
        return
    # The child was launched in its own new session. Never signal by name,
    # and never touch processes outside this run's owned process group.
    try:
        os.killpg(proc.pid, signal.SIGTERM)
    except ProcessLookupError:
        return
    try:
        proc.wait(timeout=2)
    except subprocess.TimeoutExpired:
        if proc.poll() is None:
            try:
                os.killpg(proc.pid, signal.SIGKILL)
            except ProcessLookupError:
                return
            proc.wait(timeout=2)


def process_rss_kib(pid: int) -> int | None:
    try:
        for line in Path(f"/proc/{pid}/status").read_text().splitlines():
            if line.startswith("VmRSS:"):
                return int(line.split()[1])
    except (FileNotFoundError, PermissionError, ValueError):
        pass
    return None


def capture(grim: str, env: dict[str, str], image_path: Path) -> dict[str, object]:
    from PIL import Image

    try:
        result = subprocess.run(
            [grim, str(image_path)], env=env, stdin=subprocess.DEVNULL,
            capture_output=True, timeout=4, check=False,
        )
    except subprocess.TimeoutExpired:
        return {"captured": False, "reason": "grim timeout"}
    if result.returncode != 0:
        return {"captured": False, "reason": f"grim exit {result.returncode}"}
    try:
        with Image.open(image_path) as image:
            image.load()
            rgb = image.convert("RGB")
            # Low sampling overhead, still far above a one-pixel false positive.
            hits = []
            for y in range(0, rgb.height, 3):
                for x in range(0, rgb.width, 3):
                    r, g, b = rgb.getpixel((x, y))
                    if r < 50 and g > 210 and b < 70:
                        hits.append((x, y))
            green = len(hits)
            bounds = ([min(x for x, _ in hits), min(y for _, y in hits),
                       max(x for x, _ in hits), max(y for _, y in hits)] if hits else None)
            return {"captured": True, "width": rgb.width, "height": rgb.height,
                    "oracle_green_samples": green, "oracle_bounds_px": bounds,
                    "page_pixels_verified": green >= 100}
    except (OSError, ValueError) as exc:
        return {"captured": False, "reason": f"invalid screenshot: {type(exc).__name__}"}


def execute(args: argparse.Namespace, checked: dict[str, object]) -> tuple[int, dict[str, object]]:
    if args.output_dir:
        output = Path(args.output_dir).expanduser().absolute()
        # Exclusive creation: do not overwrite artifacts from another run.
        output.mkdir(mode=0o700, parents=False, exist_ok=False)
    else:
        output = Path(tempfile.mkdtemp(prefix="nura-browser-nested-e2e-"))
    evidence: dict[str, object] = {
        "status": "FAIL", "output_dir": str(output),
        "pixel": "PENDING", "keyboard_focus": "PENDING",
        "touch": "PENDING", "tab_geometry": "PENDING",
        "first_frame_ms": None, "rss_kib": {},
    }
    processes: list[subprocess.Popen[bytes]] = []
    server: ThreadingHTTPServer | None = None
    try:
        with tempfile.TemporaryDirectory(prefix="runtime-", dir=output) as runtime_s:
            runtime = Path(runtime_s)
            runtime.chmod(0o700)
            env = os.environ.copy()
            for key in ("SWAYSOCK", "I3SOCK", "DISPLAY", "WAYLAND_DISPLAY",
                        "WAYLAND_SOCKET", "WPE_PLATFORM", "WLR_DRM_DEVICES",
                        "DBUS_SESSION_BUS_ADDRESS"):
                env.pop(key, None)
            env.update({"XDG_RUNTIME_DIR": str(runtime), "WLR_BACKENDS": "headless",
                        "WLR_HEADLESS_OUTPUTS": "1", "WLR_RENDERER": "pixman",
                        "WLR_LIBINPUT_NO_DEVICES": "1", "XDG_SESSION_TYPE": "wayland"})
            for subdir, name in (("cache", "XDG_CACHE_HOME"), ("data", "XDG_DATA_HOME"),
                                 ("state", "XDG_STATE_HOME"), ("config", "XDG_CONFIG_HOME")):
                isolated = runtime / subdir
                isolated.mkdir(mode=0o700)
                env[name] = str(isolated)
            compositor_name = str(checked["compositor"])
            if compositor_name == "sway":
                config = runtime / "sway.conf"
                config.write_text("xwayland disable\noutput * resolution 1024x768\n"
                                  "default_border none\n", encoding="utf-8")
                command = [str(checked["compositor_bin"]), "-c", str(config)]
            else:
                config_dir = runtime / "labwc"
                config_dir.mkdir(mode=0o700)
                (config_dir / "rc.xml").write_text(
                    "<labwc_config><core><gap>0</gap></core></labwc_config>\n", encoding="utf-8")
                command = [str(checked["compositor_bin"]), "--config", str(config_dir)]
            compositor = owned_process(command, env, output / "compositor.log")
            processes.append(compositor)
            display = wait_for_socket(runtime, compositor, 8)
            if display is None:
                evidence["reason"] = "nested compositor failed to create a private socket"
                return 1, evidence
            env["WAYLAND_DISPLAY"] = display
            evidence["nested_socket"] = display

            server = ThreadingHTTPServer(("127.0.0.1", 0), OracleHandler)
            threading.Thread(target=server.serve_forever, daemon=True).start()
            uri = f"http://127.0.0.1:{server.server_port}/"
            start = time.monotonic()
            engine = owned_process([str(checked["engine_bin"]), "--platform=wayland",
                                    "--url=" + uri], env, output / "engine.log")
            processes.append(engine)
            time.sleep(0.5)
            if engine.poll() is not None:
                evidence["reason"] = f"WPE engine exited {engine.returncode} before UI"
                return 1, evidence
            browser = owned_process([str(checked["browser_bin"])], env, output / "browser.log")
            processes.append(browser)
            deadline = time.monotonic() + args.timeout
            shot = output / "first-page.png"
            while time.monotonic() < deadline:
                if browser.poll() is not None or engine.poll() is not None:
                    evidence["reason"] = "browser or engine exited before first oracle frame"
                    break
                measured = capture(str(checked["grim_bin"]), env, shot)
                if measured.get("page_pixels_verified"):
                    evidence["pixel"] = "PASS"
                    evidence["screenshot"] = str(shot)
                    evidence["pixel_measurement"] = measured
                    evidence["first_frame_ms"] = round((time.monotonic() - start) * 1000, 1)
                    break
                time.sleep(0.25)
            if evidence["pixel"] != "PASS":
                evidence.setdefault("reason", "no confirmed WPE pixel-oracle frame before deadline")
            else:
                key_result = subprocess.run([str(checked["wtype_bin"]), "-k", "Tab"],
                                            env=env, stdin=subprocess.DEVNULL,
                                            capture_output=True, timeout=4, check=False)
                evidence["keyboard_injection_exit"] = key_result.returncode
                evidence["keyboard_focus"] = "PENDING"  # Key delivery alone is not focus proof.
                evidence["keyboard_after"] = capture(str(checked["grim_bin"]), env,
                                                     output / "after-key.png")
            evidence["rss_kib"] = {
                "compositor": process_tree_rss(compositor.pid),
                "engine_webprocess_tree": process_tree_rss(engine.pid),
                "browser": process_tree_rss(browser.pid),
            }
            # No virtual touch protocol trace; never label a pointer tap as touch.
            evidence["status"] = "PARTIAL" if evidence["pixel"] == "PASS" else "FAIL"
            return 1, evidence
    except (OSError, subprocess.TimeoutExpired) as exc:
        evidence["reason"] = f"{type(exc).__name__}: {exc}"
        return 1, evidence
    finally:
        for proc in reversed(processes):
            stop_owned(proc)
        if server is not None:
            server.shutdown()
            server.server_close()
        (output / "evidence.json").write_text(
            json.dumps(evidence, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--probe", action="store_true", help="read-only dependency probe")
    parser.add_argument("--output-dir", help="new private directory for run artifacts")
    parser.add_argument("--browser-bin", default=str(DEFAULT_BROWSER))
    parser.add_argument("--engine-bin", default="nura-browser-engine")
    parser.add_argument("--compositor", choices=("auto", "sway", "labwc"), default="auto")
    parser.add_argument("--timeout", type=float, default=12)
    args = parser.parse_args()
    if not (1 <= args.timeout <= 60):
        parser.error("--timeout must be between 1 and 60 seconds")
    checked = prereqs(args)
    if args.probe or checked["missing"]:
        print(json.dumps(checked, indent=2, sort_keys=True))
        return 77 if checked["missing"] else 0
    try:
        code, result = execute(args, checked)
    except FileExistsError:
        parser.error("output directory exists: refusing to overwrite artifacts")
    print(json.dumps(result, indent=2, sort_keys=True))
    return code


if __name__ == "__main__":
    raise SystemExit(main())
