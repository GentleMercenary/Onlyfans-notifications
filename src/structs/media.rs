use std::path::Path;
use serde::Deserialize;
use chrono::{DateTime, Utc};
use futures::future::join_all;
use filetime::{set_file_mtime, FileTime};
use crate::{client::{OFClient, Authorized}, deserializers::str_to_date};

#[derive(Deserialize, Debug)]
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
	#[serde(deserialize_with = "str_to_date")]
	created_at: DateTime<Utc>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Message {
	id: u64,
	#[serde(rename = "type")]
	media_type: MediaType,
	src: Option<String>,
	preview: Option<String>,
	can_view: bool,
	#[serde(default = "Utc::now")]
	#[serde(deserialize_with = "str_to_date")]
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
	#[serde(deserialize_with = "str_to_date")]
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

impl Media for Message {
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
	fn thumbnail(&self) -> Option<&str> { None }
	fn media_type(&self) -> &MediaType { &MediaType::Photo }
	fn unix_time(&self) -> i64 { Utc::now().timestamp() }
}

struct CommonMedia<'a> {
	source: Option<&'a str>,
	thumbnail: Option<&'a str>,
}

impl<'a, M: Media> From<&'a M> for CommonMedia<'a> {
	fn from(value: &'a M) -> Self {
		CommonMedia { source: value.source(), thumbnail: value.thumbnail() }
	}
}

pub async fn download_media<T: Media>(client: &OFClient<Authorized>, media: &[T], path: &Path) {
	join_all(media.iter().filter_map(|media| {
		let type_str = match media.media_type() {
			MediaType::Photo => "Images",
			MediaType::Audio => "Audios",
			MediaType::Video | MediaType::Gif => "Videos",
		};

		CommonMedia::from(media).source.map(|url| async move {
			client.fetch_file(url, &path.join(type_str), None)
			.await
			.inspect_err(|err| error!("Download failed: {err}"))
			.map(|(downloaded, path)| {
				if downloaded {
					let _ = set_file_mtime(path, FileTime::from_unix_time(media.unix_time(), 0))
						.inspect_err(|err| warn!("Error setting file modify time: {err}"));
				}
			})
		})
	}))
	.await;
}