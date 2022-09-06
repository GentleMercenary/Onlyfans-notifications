use std::{collections::HashMap};

use serde::de::Error;
use chrono::{DateTime, Utc};
use serde::{Deserializer, Deserialize};
use strip_markdown::*;

use super::client::Cookie;

pub fn str_to_date<'de, D>(deserializer: D) -> Result<DateTime<Utc>, D::Error>
where
	D: Deserializer<'de>,
{
	let s: &str = Deserialize::deserialize(deserializer)?;
	Ok(DateTime::parse_from_rfc3339(s)
		.map(|date| date.with_timezone(&Utc))
		.unwrap())
}

pub fn de_markdown_string<'de, D>(deserializer: D) -> Result<String, D::Error>
where
	D: Deserializer<'de>,
{
	String::deserialize(deserializer)
	.map(|s| strip_markdown(&s))
}

pub fn parse_cookie<'de, D>(deserializer: D) -> Result<Cookie, D::Error>
where
	D: Deserializer<'de>,
{
	let s = non_empty_str(deserializer)?;
	let mut cookie_map: HashMap<&str, &str> = HashMap::new();
	let filtered_str = s.replace(';', "");
	for c in filtered_str.split(' ') {
		let (k, v) = c.split_once('=').expect("Key/Value cookie pair found");
		cookie_map.insert(
			k,
			v
		);
	}

	Ok(Cookie {
		auth_id: cookie_map.get("auth_id").ok_or("'auth_id' missing from cookie auth parameter").map_err(D::Error::custom)?.to_string(),
		sess: cookie_map.get("sess").ok_or("'sess' missing from cookie auth parameter").map_err(D::Error::custom)?.to_string(),
		auth_hash: cookie_map.get("auth_hash").unwrap_or(&"").to_string(),
	})
}

pub fn non_empty_str<'de, D>(deserializer: D) -> Result<&'de str, D::Error>
where
	D: Deserializer<'de>,
{
	let s: &str = Deserialize::deserialize(deserializer)?;
	(!s.is_empty())
	.then_some(s)
	.ok_or("Empty string").map_err(D::Error::custom)

}

pub fn non_empty_string<'de, D>(deserializer: D) -> Result<String, D::Error>
where
	D: Deserializer<'de>,
{
	non_empty_str(deserializer)
	.map(|s| s.to_owned())
}