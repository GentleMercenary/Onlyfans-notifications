#![feature(let_chains)]

pub mod socket;
pub mod deserializers;
pub mod helpers;
pub mod handlers;
pub mod settings;
pub mod structs;

#[macro_use]
extern crate log;

use std::{fs, io};
use of_client::client::AuthParams;
use serde::Deserialize;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum FileParseError {
	#[error("{0}")]
	IO(#[from] io::Error),
	#[error("{0}")]
	Parse(#[from] serde_json::Error)
}

pub fn get_auth_params() -> Result<AuthParams, FileParseError> {
	#[derive(Deserialize)]
	struct _AuthParams { auth: AuthParams }

	fs::read_to_string("auth.json")
	.inspect_err(|err| error!("Error reading auth file: {err}"))
	.and_then(|data| serde_json::from_str::<_AuthParams>(&data).map_err(Into::into))
	.inspect_err(|err| error!("Error parsing auth data: {err}"))
	.map(|params| params.auth)
	.inspect(|params| debug!("{params:?}"))
	.map_err(Into::into)
}