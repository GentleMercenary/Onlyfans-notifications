use crate::client::Cookie;

use serde::de::Error;
use std::collections::HashMap;
use serde::{Deserialize, Deserializer};

impl<'de> Deserialize<'de> for Cookie {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>
	{
		let s: &str = Deserialize::deserialize(deserializer)?;
		let mut cookie_map: HashMap<String, String> = HashMap::new();
		let filtered_str = s.replace(';', " ");
		for c in filtered_str.split_whitespace() {
			let (k, v) = c
				.split_once('=')
				.ok_or_else(|| D::Error::custom("'Key=Value' cookie pair pattern not found"))?;
			cookie_map.insert(k.to_string(), v.to_string());
		}
	
		let auth_id = cookie_map
			.remove("auth_id")
			.ok_or_else(|| D::Error::custom("'auth_id' missing from cookie"))?;
	
		let sess = cookie_map
			.remove("sess")
			.ok_or_else(|| D::Error::custom("'sess' missing from cookie"))?;
	
		Ok(Cookie {
			auth_id,
			sess,
			other: cookie_map
	
		})
	}
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