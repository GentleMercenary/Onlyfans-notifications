#![allow(dead_code)]

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
pub struct DRMSignature {
	#[serde(rename = "CloudFront-Policy")]
	pub policy: String,
	#[serde(rename = "CloudFront-Signature")]
	pub signature: String,
	#[serde(rename = "CloudFront-Key-Pair-Id")]
	pub key_pair: String
}

#[derive(Deserialize, Debug)]
pub struct DRMKVP<T> {
	pub hls: T,
	pub dash: T
}

#[derive(Deserialize, Debug)]
pub struct DRM {
	pub manifest: DRMKVP<String>,
	pub signature: DRMKVP<DRMSignature>
}

#[derive(Deserialize, Debug)]
pub(crate) struct Files {
	full: File,
	preview: Option<File>,
	pub(crate) drm: Option<DRM>
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Feed {
	pub id: u64,
	#[serde(rename = "type")]
	media_type: MediaType,
	pub(crate) files: Files,
	can_view: bool,
	created_at: Option<DateTime<Utc>>,
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

	#[cfg(feature = "drm")]
	fn drm(&self) -> Option<&DRM>;
}

impl Media for Feed {
	fn source(&self) -> Option<&str> { self.files.full.url.as_deref() }
	fn thumbnail(&self) -> Option<&str> { self.files.preview.as_ref().and_then(|preview: &File| preview.url.as_deref()) }
	fn media_type(&self) -> &MediaType { &self.media_type }
	fn unix_time(&self) -> i64 { self.created_at.unwrap_or_else(Utc::now).timestamp() }

	#[cfg(feature = "drm")]
	fn drm(&self) -> Option<&DRM> { self.files.drm.as_ref() }
}

impl Media for Stream {
	fn source(&self) -> Option<&str> { None }
	fn thumbnail(&self) -> Option<&str> { Some(&self.thumb_url) }
	fn media_type(&self) -> &MediaType { &MediaType::Photo }
	fn unix_time(&self) -> i64 { Utc::now().timestamp() }

	#[cfg(feature = "drm")]
	fn drm(&self) -> Option<&DRM> { None }
}