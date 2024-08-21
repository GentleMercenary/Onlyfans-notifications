use crate::deserializers::de_str_to_date;

use serde::Deserialize;
use chrono::{DateTime, Utc};

#[derive(Deserialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MediaType {
	Photo,
	Video,
	Gif,
	Audio,
}

#[derive(Deserialize, Debug)]
struct File {
	url: Option<String>,
}

#[derive(Deserialize, Debug)]
struct Files {
	full: File,
	preview: Option<File>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Feed {
	id: u64,
	#[serde(rename = "type")]
	media_type: MediaType,
	files: Files,
	can_view: bool,
	#[serde(default = "Utc::now")]
	#[serde(deserialize_with = "de_str_to_date")]
	created_at: DateTime<Utc>,
}

// TODO: actually make use of this
#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Stream {
	thumb_url: String,
}

pub trait Media {
	fn source(&self) -> Option<&str>;
	fn thumbnail(&self) -> Option<&str>;
	fn media_type(&self) -> &MediaType;
	fn unix_time(&self) -> i64;
}

impl Media for Feed {
	fn source(&self) -> Option<&str> { self.files.full.url.as_deref() }
	fn thumbnail(&self) -> Option<&str> { self.files.preview.as_ref().and_then(|preview: &File| preview.url.as_deref()) }
	fn media_type(&self) -> &MediaType { &self.media_type }
	fn unix_time(&self) -> i64 { self.created_at.timestamp() }
}

impl Media for Stream {
	fn source(&self) -> Option<&str> { None }
	fn thumbnail(&self) -> Option<&str> { Some(&self.thumb_url) }
	fn media_type(&self) -> &MediaType { &MediaType::Photo }
	fn unix_time(&self) -> i64 { Utc::now().timestamp() }
}