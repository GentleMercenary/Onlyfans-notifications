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
#[serde(rename_all = "camelCase")]
pub struct Post {
	id: u64,
	#[serde(rename = "type")]
	media_type: MediaType,
	full: Option<String>,
	preview: Option<String>,
	can_view: bool,
	#[serde(default = "Utc::now")]
	#[serde(deserialize_with = "de_str_to_date")]
	created_at: DateTime<Utc>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Chat {
	id: u64,
	#[serde(rename = "type")]
	media_type: MediaType,
	src: Option<String>,
	preview: Option<String>,
	can_view: bool,
	#[serde(default = "Utc::now")]
	#[serde(deserialize_with = "de_str_to_date")]
	created_at: DateTime<Utc>,
}

#[derive(Deserialize, Debug)]
struct __Files {
	url: Option<String>,
}

#[derive(Deserialize, Debug)]
struct _Files {
	source: __Files,
	preview: __Files,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Story {
	id: u64,
	#[serde(rename = "type")]
	media_type: MediaType,
	files: _Files,
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

impl Media for Post {
	fn source(&self) -> Option<&str> { self.full.as_deref() }
	fn thumbnail(&self) -> Option<&str> { self.preview.as_deref() }
	fn media_type(&self) -> &MediaType { &self.media_type }
	fn unix_time(&self) -> i64 { self.created_at.timestamp() }
}

impl Media for Chat {
	fn source(&self) -> Option<&str> { self.src.as_deref() }
	fn thumbnail(&self) -> Option<&str> { self.preview.as_deref() }
	fn media_type(&self) -> &MediaType { &self.media_type }
	fn unix_time(&self) -> i64 { self.created_at.timestamp() }
}

impl Media for Story {
	fn source(&self) -> Option<&str> { self.files.source.url.as_deref() }
	fn thumbnail(&self) -> Option<&str> { self.files.preview.url.as_deref() }
	fn media_type(&self) -> &MediaType { &self.media_type }
	fn unix_time(&self) -> i64 { self.created_at.timestamp() }
}

impl Media for Stream {
	fn source(&self) -> Option<&str> { None }
	fn thumbnail(&self) -> Option<&str> { Some(&self.thumb_url) }
	fn media_type(&self) -> &MediaType { &MediaType::Photo }
	fn unix_time(&self) -> i64 { Utc::now().timestamp() }
}