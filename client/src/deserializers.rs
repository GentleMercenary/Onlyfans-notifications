use crate::client::Cookie;

use serde::de::Error;
use chrono::{DateTime, ParseResult, Utc};
use std::collections::HashMap;
use strip_markdown::strip_markdown;
use serde::{Deserialize, Deserializer};

pub fn str_to_date(s: &str) -> ParseResult<DateTime<Utc>> {
	DateTime::parse_from_rfc3339(s)
	.map(|date| date.with_timezone(&Utc))
}

pub fn de_str_to_date<'de, D>(deserializer: D) -> Result<DateTime<Utc>, D::Error>
where
	D: Deserializer<'de>,
{
	let s: &str = Deserialize::deserialize(deserializer)?;
	str_to_date(s)
	.map_err(D::Error::custom)	
}

pub fn de_markdown_string<'de, D>(deserializer: D) -> Result<String, D::Error>
where
	D: Deserializer<'de>,
{
	String::deserialize(deserializer).map(|s| strip_markdown(&s))
}

pub fn parse_cookie<'de, D>(deserializer: D) -> Result<Cookie, D::Error>
where
	D: Deserializer<'de>,
{
	let s = non_empty_str(deserializer)?;
	let mut cookie_map: HashMap<String, String> = HashMap::new();
	let filtered_str = s.replace(';', "");
	for c in filtered_str.split(' ') {
		let (k, v) = c
			.split_once('=')
			.ok_or_else(|| D::Error::custom("No Key/Value cookie pair found"))?;
		cookie_map.insert(k.to_string(), v.to_string());
	}

	let auth_id = cookie_map
		.remove("auth_id")
		.ok_or_else(|| D::Error::custom("'auth_id' missing from cookie auth parameter"))?;

	let sess = cookie_map
		.remove("sess")
		.ok_or_else(|| D::Error::custom("'sess' missing from cookie auth parameter"))?;

	Ok(Cookie {
		auth_id,
		sess,
		other: cookie_map

	})
}

pub fn non_empty_str<'de, D>(deserializer: D) -> Result<&'de str, D::Error>
where
	D: Deserializer<'de>,
{
	let s: &str = Deserialize::deserialize(deserializer)?;
	(!s.is_empty())
	.then_some(s)
	.ok_or_else(|| D::Error::custom("Empty string"))
}

pub fn non_empty_string<'de, D>(deserializer: D) -> Result<String, D::Error>
where
	D: Deserializer<'de>,
{
	non_empty_str(deserializer).map(str::to_owned)
}