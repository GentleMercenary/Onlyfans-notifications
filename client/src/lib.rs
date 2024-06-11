#![feature(let_chains)]

#[macro_use]
extern crate log;

pub mod structs;
pub mod client;
pub mod deserializers;

pub use structs::content;
pub use structs::media;
pub use structs::user;
pub use reqwest::Url;
pub use reqwest::IntoUrl;