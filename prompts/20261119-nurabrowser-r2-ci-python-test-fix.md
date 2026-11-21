# Repair browser Meson contract test and register full CI coverage

## Evidence
OCP root-cause 6888ED clean-source browser Python inventory ran 70 tests, 1 failure and 1 skip. components/nura-browser-engine/tests/test_offline_qualification.py::BuildContractTests::test_explicit_pkg_config_linkage asserts obsolete "rename: ',browse'" even though current Meson declares a private executable 'nurabrowser-core'. Main CI currently omits some full browser Python tests. Another active lane owns .github/workflows/nurabrowser-engine-armhf.yml and scripts/test_browser_engine_ci.py: coordinate and never modify those files without handoff.

## Scope
Read AGENTS.md, DESIGN.md, relevant browser architecture and prior CI checkpoint. In: components/nura-browser-engine/tests/test_offline_qualification.py, .github/workflows/ci.yml only after claim, a focused CI workflow coverage regression test, docs/qualification/NURA-BROWSER-R2-CI-PYTHON-CLOSE.md. Out: ARM gate2 audit and Meson executable implementation, private release artifacts, device/remote CI/Git push.

## Acceptance
Replace obsolete rename assertion with a stronger exact private executable/install/absence-public-alias contract; do not weaken ABI or security checks. Full browser Python suite passes locally, CI test suite enumerates these tests without blanket skip, native packaging provenance tests remain green, and all modifications are reviewable with exact exit codes/checkpoint.
