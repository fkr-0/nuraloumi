@projmgrauth
Set repo: nuraloumi

# NuraLoumi OCP Reconciliation

Canonical OCP task: NURALOUMI-OCP-RECONCILE-20261002
Priority: 98
Lane: control-plane

Mission: make canonical OCP state accurately reflect reviewed project reality without inventing completion.

Already-approved Wave-1 Core/Cairo/Shell packets must reconcile their stale canonical tasks to terminal complete using exact PhaseResult and review authority refs. Providers, Wayland and Xtask should remain terminal complete.

Inspect ready runtime packets. For every ready packet whose exact objective is already demonstrably landed and independently reviewed, retire/supersede it using supported lifecycle mechanisms; do not cancel or close the genuine live-search task or any work whose acceptance is not actually met. Preserve provenance and record why each packet is superseded.

Known likely superseded ready objectives:
- menu integration R2
- ARMv7 Cairo pkg-config defaults if current config/tooling already proves it
- land reviewed provider implementation
- foreign-toplevel Wayland backend
- strict-font deployment workflow

Keep the live top-bar search field ready until actually implemented/reviewed.

Acceptance: no stale running Wave-1 Core/Cairo/Shell tasks; no duplicate ready work for already-landed reviewed objectives; searchbar remains actionable; discrepancies documented rather than force-mutated.
