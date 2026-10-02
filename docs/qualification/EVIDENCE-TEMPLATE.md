# SL101 qualification evidence template

## Identity

- date/time:
- git commit:
- package manifest SHA-256:
- target triple: armv7-unknown-linux-musleabihf
- tablet/kernel:
- Nura userspace release:
- compositor and renderer:
- recovery path confirmed before test: yes/no

## Compiler and ELF

- Rust target installed: PASS/PENDING
- cross linker present: PASS/PENDING
- cargo cross check: PASS/PENDING/FAIL
- release binaries produced: PASS/PENDING/FAIL
- ELF ARM32 audit: PASS/PENDING/FAIL
- NEON attribute/disassembly heuristic: PASS/PENDING/FAIL
- exact commands/log paths:

## Startup / no-SIGILL

- panel start: PASS/FAIL
- menu start: PASS/FAIL
- SIGILL observed: yes/no
- first interactive frame latency ms:
- raw timing evidence:

## Touch

Record all four corners after output transform.

| Corner | Expected | Observed | PASS/FAIL |
| --- | --- | --- | --- |
| top-left | top-left | | |
| top-right | top-right | | |
| bottom-left | bottom-left | | |
| bottom-right | bottom-right | | |

- drag/motion direction:
- transform/config:

## Physical keyboard

- open menu:
- up/down wrap and skip non-action rows:
- right enters submenu:
- left/back exits submenu:
- Enter activates same semantic action as touch:
- Escape closes level/surface:
- PASS/FAIL and notes:

## Resources

| State | panel RSS KiB | menu RSS KiB | combined RSS KiB | CPU % | notes |
| --- | ---: | ---: | ---: | ---: | --- |
| baseline | | | | | |
| idle 60 s | | | | | |
| menu open | | | | | |
| after 100 cycles | | | | | |

- idle combined RSS <= 35 MiB: PASS/FAIL
- incremental menu RSS <= 12 MiB: PASS/FAIL
- no continuous idle redraw observed: PASS/FAIL

## 100x menu open/close

- harness/commands:
- starting panel PID:
- ending panel PID:
- starting labwc PID:
- ending labwc PID:
- crashes/errors:
- memory growth:
- PASS/FAIL:

## Suspend / resume

- state before suspend:
- suspend method:
- resume result:
- panel survived or restarted cleanly:
- touch after resume:
- keyboard after resume:
- PASS/FAIL:

## Failure-state checks

- network service absent/unavailable:
- audio service absent/unavailable:
- brightness unavailable/read-only:
- provider errors shown without shell/compositor failure:
- PASS/FAIL:

## Rollback

- NuraLoumi processes terminated:
- labwc remained/recovered:
- baseline desktop usable:
- no /usr changes:
- no boot/service enablement:
- staging removal optional and sufficient:
- PASS/FAIL:

## Final evidence classification

- host qualification: PASS/PENDING/FAIL
- cross-build qualification: PASS/PENDING/FAIL
- real SL101 qualification: PASS/PENDING/FAIL
- remaining blockers:
