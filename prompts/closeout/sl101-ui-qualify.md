@projmgrauth
Set repo: nuraloumi

# NuraLoumi SL101 UI Qualification

Canonical OCP task: NURALOUMI-SL101-UI-QUALIFY-20261002
Priority: 92
Lane: device-ui

Dependency: the current Cairo density/screenshot fixup packets must be landed and reviewed first. Avoid broad redesign.

Qualify the actual live SL101 UI with evidence:
- 40 logical px top bar;
- 48 logical px actionable menu rows/hit regions;
- 448 logical px nominal menu width with safe edge clamping;
- packaged-font readability and strict-font path;
- long labels/subtitles/shortcut text and ellipsis;
- menus near all relevant display edges;
- normal and rotated coordinate mapping;
- touch hit/release behavior;
- physical slider keyboard navigation/activation/back;
- searchbar if landed;
- screenshot/evidence set before and after only narrowly justified polish changes.

Any fix must be minimal, target-specific, and keep software rendering/labwc-pixman fallback intact.

Acceptance: screenshot/evidence pack plus automated geometry/text tests and live touch/keyboard proof; no regression of >=48px actionable targets; independent review.
