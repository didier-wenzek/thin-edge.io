//! A library to define, compose and run actors
//!
//! Actors are processing units that interact using asynchronous messages.

mod channels;
mod errors;
mod messages;

pub use channels::*;
pub use errors::*;
pub use messages::*;