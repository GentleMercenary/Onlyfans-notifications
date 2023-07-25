#![feature(result_option_inspect)]
#![feature(let_chains)]

#[macro_use]
extern crate log;

pub mod structs;
pub mod client;
mod deserializers;

pub use structs::content;
pub use structs::media;
pub use structs::user;