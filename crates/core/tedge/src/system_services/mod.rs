//! Common interface to the system-provided _service management facility_ to start, stop, enable,
//! disable or query the status of system services.
//!
//! Supported service management facilities include:
//!
//! * systemd
//! * OpenRC
//! * `service(8)` as found on BSDs.
//!

mod command_builder;
mod error;
mod manager;
mod manager_ext;
mod managers;
mod services;

pub use self::command_builder::*;
pub use self::error::*;
pub use self::manager::*;
pub use self::manager_ext::*;
pub use self::managers::*;
pub use self::services::*;
