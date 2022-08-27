use std::collections::HashMap;

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
	let s: &str = Deserialize::deserialize(deserializer)?;
	let mut cookie_map: HashMap<&str, &str> = HashMap::new();
	let filtered_str = s.replace(';', "");
	for c in filtered_str.split(' ') {
		let mut split_cookie = c.split('=');
		cookie_map.insert(
			split_cookie.next().unwrap(),
			split_cookie.next().unwrap()
		);
	}

	Ok(Cookie {
		auth_id: cookie_map.get("auth_id").unwrap_or(&"").to_string(),
		sess: cookie_map.get("sess").unwrap_or(&"").to_string(),
		auth_hash: cookie_map.get("auth_hash").unwrap_or(&"").to_string(),
	})
}