use std::{str::FromStr, fmt::Display};

use chrono::{DateTime, Utc};
use log::LevelFilter;
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

pub fn from_string_vec<'de, T, D>(deserializer: D) -> Result<Vec<T>, D::Error>
where
	D: Deserializer<'de>,
	T: FromStr + serde::Deserialize<'de>,
	<T as FromStr>::Err: Display,
{
	Deserialize::deserialize(deserializer)
	.and_then(|s: Vec<String>|
		s.iter().map(|v|
			v.parse::<T>().map_err(serde::de::Error::custom)
		).collect()
	)
}

pub fn de_str_to_date_opt<'de, D>(deserializer: D) -> Result<Option<DateTime<Utc>>, D::Error>
where
	D: Deserializer<'de>
{

	Deserialize::deserialize(deserializer)
	.and_then(|s: Option<&str>|
		s
		.map(str_to_date)
		.transpose()
		.map_err(serde::de::Error::custom)
	)
}

pub fn de_log_level<'de, D>(deserializer: D) -> Result<LevelFilter, D::Error>
where
	D: Deserializer<'de>
{
	let s: &str = Deserialize::deserialize(deserializer)?;
	match s {
		"TRACE" | "Trace" | "trace" => Ok(LevelFilter::Trace),
		"DEBUG" | "Debug" | "debug" => Ok(LevelFilter::Debug),
		"INFO" | "Info" | "info" => Ok(LevelFilter::Info),
		"WARN" | "Warn" | "warn" => Ok(LevelFilter::Warn),
		"ERROR" | "Error" | "error" => Ok(LevelFilter::Error),
		"OFF" | "Off" | "off" => Ok(LevelFilter::Off),
		_ => Err(serde::de::Error::custom("Unkown filter level"))
	}
}