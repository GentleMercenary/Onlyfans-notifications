#![feature(let_chains)]

pub mod socket;
pub mod deserializers;
pub mod helpers;
pub mod handlers;
pub mod settings;
pub mod structs;

#[macro_use]
extern crate log;

use crate::deserializers::non_empty_str;
use std::{fs, io};
use cookie::{Cookie, ParseError};
use of_client::{client::AuthParams, reqwest_cookie_store::CookieStore};
use reqwest::Url;
use serde::Deserialize;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum FileParseError {
	#[error("{0}")]
	IO(#[from] io::Error),
	#[error("{0}")]
	Parse(#[from] serde_json::Error)
}

#[derive(Error, Debug)]
pub enum AuthParseError {
	#[error("{0}")]
	IO(#[from] io::Error),
	#[error("{0}")]
	Parse(#[from] serde_json::Error),
	#[error("{0}")]
	CookieParse(#[from] ParseError),
	#[error("Cookie is missing '{0}' field")]
	IncompleteCookie(String)
}

pub fn get_auth_params() -> Result<AuthParams, AuthParseError> {
	#[derive(Debug, Deserialize)]
	struct AuthFileInner<'a> {
		#[serde[borrow]]
		#[serde(deserialize_with = "non_empty_str")]
		cookie: &'a str,
		#[serde[borrow]]
		#[serde(deserialize_with = "non_empty_str")]
		user_agent: &'a str,
		#[serde[borrow]]
		#[serde(deserialize_with = "non_empty_str")]
		x_bc: &'a str
	}

	#[derive(Deserialize)]
	struct AuthFile<'a> { #[serde(borrow)] auth: AuthFileInner<'a> }

	let data = fs::read_to_string("auth.json")
		.inspect_err(|err| error!("Error reading auth file: {err}"))?;

	let parsed = serde_json::from_str::<AuthFile>(&data)
		.inspect_err(|err| error!("Error parsing auth data: {err}"))?
		.auth;

	let mut store = CookieStore::new(None);
	let url: Url = "https://onlyfans.com".parse().unwrap();
	for cookie in Cookie::split_parse(parsed.cookie) {
		match cookie {
			Ok(cookie) => {
				let _ = store.insert_raw(&cookie, &url)
				.inspect_err(|err| warn!("{err}"));
			}
			Err(err) => {
				error!("{err}");
				return Err(AuthParseError::CookieParse(err))
			}
		}
	}
	
	if !store.contains_any(url.domain().unwrap(), url.path(), "sess") {
		return Err(AuthParseError::IncompleteCookie("sess".to_string()))
			.inspect_err(|err| error!("{err}"))
	}

	let user_id = store.get_any(url.domain().unwrap(), url.path(), "auth_id")
		.map(|cookie| cookie.value().to_string())
		.ok_or_else(|| AuthParseError::IncompleteCookie("auth_id".to_string()))
		.inspect_err(|err| error!("{err}"))?;

	Ok(AuthParams {
		cookie: store,
		user_id,
		x_bc: parsed.x_bc.to_string(),
		user_agent: parsed.user_agent.to_string()
	})
}