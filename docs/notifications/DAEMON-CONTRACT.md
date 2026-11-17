# NuraLoumi notification daemon integration contract

## Root cause and authority

The previous `NotificationProvider` in `crates/nuraloumi-providers/src/notifications.rs` was a passive Dunst `dunstctl history` observer. `crates/nuraloumi-shell/src/bin/toast.rs` explicitly implements a one-shot, caller-driven Wayland popup. Neither acquired `org.freedesktop.Notifications`; a `notify-send` request therefore failed when no other daemon or D-Bus activation service was installed.

`nuraloumi-notificationd` is now the authoritative standalone session-bus service. It claims `org.freedesktop.Notifications` with **DoNotQueue**, without AllowReplacement/ReplaceExisting. If Dunst or another owner exists it must exit nonzero. The core daemon deliberately does not mutate an existing compositor session or auto-install itself.

## Lane 14 installer ownership

Packaging lane 14 owns the installation and autostart artifacts and may choose **one** activation strategy per user session:

1. D-Bus activation file: `$prefix/share/dbus-1/services/org.freedesktop.Notifications.service` with:
   `[D-BUS Service]`
   `Name=org.freedesktop.Notifications`
   `Exec=$prefix/bin/nuraloumi-notificationd`
2. Or user-session service: `nuraloumi-notificationd.service` with `Type=simple` and `ExecStart=$prefix/bin/nuraloumi-notificationd`. If this replaces D-Bus activation, avoid also creating a competing XDG autostart `.desktop` launching a second instance. A systemd-activated D-Bus unit may reference `SystemdService=nuraloumi-notificationd.service` in the D-Bus service file.

Do **not** install both NuraLoumi and Dunst as concurrent automatic notification daemons. Do **not** replace or commandeer a foreign daemon. A client request via `notify-send` requires the daemon running or a correctly installed D-Bus activation file; compiling the binary is not sufficient.

The service provides standard `Notify`, `CloseNotification`, `GetCapabilities`, `GetServerInformation`, `ActionInvoked` and `NotificationClosed`, plus NuraLoumi-only `GetHistory`, `DismissHistory`, `InvokeAction`, `NotificationAdded` methods/signals on the same interface. Only the dedicated session-bus owner owns its 64-entry in-process history. The notifications center's system provider selects this backend when GetServerInformation reports NuraLoumi, otherwise falls back to Dunst history if available. This is ephemeral history, **not persisted through daemon restarts**.

## Qualification

Use `tests/notifications/private-session.sh`, which constructs a fresh `dbus-run-session` and never claims the developer's live bus. On-device test and ARMv7 package qualification remain separate release-lane gates. No SL101 deployment occurred here.

## Remaining considerations

The daemon is independent of a Wayland display and has no built-in toast rendering. `NotificationAdded` allows a toast listener to react to real arrivals, but the toast executable is still one-shot and must be wired by a separate event-consumer integration. No XDG/systemd files were changed in this lane. Do not advertise real-time toasts or production autostart until integration verifies them.
