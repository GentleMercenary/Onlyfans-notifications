use std::{str::FromStr, fmt::Display};

use august::convert_unstyled;
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

pub fn de_clean_text<'de, D>(deserializer: D) -> Result<String, D::Error>
where
	D: Deserializer<'de>,
{
	String::deserialize(deserializer).map(|s| convert_unstyled(&s, usize::MAX))
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