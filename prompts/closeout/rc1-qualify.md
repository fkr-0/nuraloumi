@projmgrauth
Set repo: nuraloumi

# NuraLoumi 0.1 RC1 Qualification

Canonical OCP task: NURALOUMI-RC1-QUALIFY-20261002
Priority: 90
Lane: release-qualification

This is the daily-usability release gate. Start only after integration closeout, live window controls, and SL101 UI qualification are complete/reviewed. Do not expand features.

Run and archive:
- clean locked release build and package/hash manifest;
- ARMv7/no-NEON instruction audit;
- copy-only SL101 deployment and simple rollback;
- 100x menu open/close soak;
- idle and menu-open RSS/CPU;
- startup/first-interactive-frame latency;
- panel restart/recovery;
- suspend/resume recovery;
- four-corner touch;
- physical slider keyboard operation;
- missing NetworkManager/audio/Bluetooth/backlight service degradation;
- windows controls and confirmation safety;
- strict packaged-font rendering;
- no EGL/GLES requirement for NuraLoumi software path;
- labwc/pixman remains usable after kill/restart/failure.

Produce PASS/FAIL/PENDING evidence. Do not call RC1 qualified if target evidence is missing.

Acceptance: reproducible RC1 bundle with hashes, exact git commit, package manifest, target evidence, resource metrics, rollback proof, and independent release review.
