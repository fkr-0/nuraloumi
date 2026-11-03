//! Linux system-state providers and explicit, validated actions.
//!
//! Snapshot calls are mutation-free. Any state change is represented by an
//! explicit action and routed through a bounded backend.

pub mod applications;
pub mod audio;
pub mod backlight;
pub mod battery;
pub mod bluetooth;
pub mod calendar;
pub mod clock;
pub mod command;
pub mod common;
pub mod error;
pub mod feeds;
pub mod maps;
pub mod media;
pub mod network;
pub mod notifications;
pub mod probe;
pub mod processes;
pub mod refresh;
pub mod renderer;
pub mod resources;
pub mod session;
pub mod weather;

pub use applications::{
    ApplicationAction, ApplicationEntry, ApplicationProvider, ApplicationSnapshot,
};
pub use audio::{AudioAction, AudioProvider, AudioSnapshot};
pub use backlight::{BacklightAction, BacklightProvider, BacklightSnapshot};
pub use battery::{BatteryProvider, BatterySnapshot};
pub use bluetooth::{BluetoothAction, BluetoothDevice, BluetoothProvider, BluetoothSnapshot};
pub use calendar::{CalendarDate, CalendarDay, CalendarEvent, CalendarProvider, CalendarSnapshot};
pub use clock::{ClockProvider, ClockSnapshot};
pub use command::{
    CommandLimits, CommandOutput, CommandRunner, CommandSpec, FixtureCommandRunner,
    SystemCommandRunner,
};
pub use common::{ActionProvider, ActionResult, Health, Provider, SnapshotMeta};
pub use error::{ProviderError, ProviderErrorCategory};
pub use media::{MediaAction, MediaPlayer, MediaProvider, MediaSnapshot};
pub use network::{NetworkAction, NetworkProvider, NetworkSnapshot, WifiNetwork};
pub use notifications::{
    NotificationAction, NotificationEntry, NotificationProvider, NotificationSnapshot,
};
pub use probe::ProbeSnapshot;
pub use processes::{ProcessEntry, ProcessProvider, ProcessSnapshot};
pub use refresh::{CachedProvider, RefreshPolicy, RefreshSnapshot};
pub use resources::{
    CpuUsage, DiskIoStats, FilesystemStats, MemoryStats, NetworkIoStats, ResourceProcess,
    ResourceProvider, ResourceSnapshot, TemperatureReading,
};
pub use session::{SessionAction, SessionProvider, SessionSnapshot};
pub use weather::{ForecastDay, WeatherProvider, WeatherSnapshot};

pub const CRATE_READY: bool = true;
