use std::{str::FromStr, fmt::Display};

use chrono::{DateTime, Utc};
use of_client::deserializers::str_to_date;
use serde::{Deserialize, Deserializer};

use crate::structs::socket;

pub fn notification_message<'de, D>(deserializer: D) -> Result<socket::Notification, D::Error>
where
	D: Deserializer<'de>,
{
	#[derive(Deserialize)]
	struct Outer {
		new_message: socket::Notification,
	}

	Outer::deserialize(deserializer).map(|outer| outer.new_message)
}

pub fn from_string<'de, T, D>(deserializer: D) -> Result<T, D::Error>
where
	D: Deserializer<'de>,
	T: FromStr + serde::Deserialize<'de>,
	<T as FromStr>::Err: Display,
{

	Deserialize::deserialize(deserializer)
	.and_then(|s: &str| s.parse::<T>().map_err(serde::de::Error::custom))
}

pub fn de_str_to_date_opt<'de, D>(deserializer: D) -> Result<Option<DateTime<Utc>>, D::Error>
where
	D: Deserializer<'de>
{
	Deserialize::deserialize(deserializer)
	.map(|s: Option<&str>| {
		s.and_then(|date: &str| str_to_date(date).ok())
	})
}