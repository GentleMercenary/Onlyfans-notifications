use std::{str::FromStr, fmt::Display};
use serde::{Deserialize, Deserializer};

use crate::structs;

pub fn notification_message<'de, D>(deserializer: D) -> Result<structs::Notification, D::Error>
where
	D: Deserializer<'de>,
{
	#[derive(Deserialize)]
	struct Outer {
		new_message: structs::Notification,
	}

	Outer::deserialize(deserializer).map(|outer| outer.new_message)
}

pub fn from_str<'de, T, D>(deserializer: D) -> Result<T, D::Error>
where
	D: Deserializer<'de>,
	T: FromStr + serde::Deserialize<'de>,
	<T as FromStr>::Err: Display,
{
	Deserialize::deserialize(deserializer)
	.and_then(|s: &str| s.parse::<T>().map_err(serde::de::Error::custom))
}

pub fn from_str_vec<'de, T, D>(deserializer: D) -> Result<Vec<T>, D::Error>
where
	D: Deserializer<'de>,
	T: FromStr + serde::Deserialize<'de>,
	<T as FromStr>::Err: Display,
{
	Deserialize::deserialize(deserializer)
	.and_then(|v: Vec<&str>| v
		.iter()
		.map(|s| s.parse::<T>().map_err(serde::de::Error::custom))
		.collect()
	)
}