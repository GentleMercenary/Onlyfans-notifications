use crate::{client::{OFClient, Authorized, AuthedClient}, deserializers::str_to_date};

use chrono::{DateTime, Utc};
use filetime::{set_file_mtime, FileTime};
use futures::future::join_all;
use serde::Deserialize;
use std::path::Path;

#[derive(Deserialize, Debug)]
struct CommonFilesInner {
	url: Option<String>,
}

#[derive(Deserialize, Debug)]
struct CommonFiles {
	source: CommonFilesInner,
	preview: CommonFilesInner,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum MediaType {
	Photo,
	Video,
	Gif,
	Audio,
}

pub struct CommonMedia<'a> {
	pub source: Option<&'a str>,
	pub thumbnail: Option<&'a str>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Media {
	id: u64,
	can_view: bool,
	#[serde(default = "Utc::now")]
	#[serde(deserialize_with = "str_to_date")]
	created_at: DateTime<Utc>,
	#[serde(rename = "type")]
	media_type: MediaType,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PostMedia {
	full: Option<String>,
	preview: Option<String>,
	#[serde(flatten)]
	shared: Media,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MessageMedia {
	src: Option<String>,
	preview: Option<String>,
	#[serde(flatten)]
	shared: Media,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct StoryMedia {
	files: CommonFiles,
	#[serde(flatten)]
	shared: Media,
}

// TODO: actually make use of this
#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct StreamMedia {
	thumb_url: String,
}

pub trait ViewableMedia {
	fn get(&self) -> CommonMedia;
	fn media_type(&self) -> &MediaType;
	fn unix_time(&self) -> i64;
}

impl ViewableMedia for PostMedia {
	fn get(&self) -> CommonMedia {
		CommonMedia {
			source: self.full.as_deref(),
			thumbnail: self.preview.as_deref(),
		}
	}

	fn media_type(&self) -> &MediaType { &self.shared.media_type }
	fn unix_time(&self) -> i64 { self.shared.created_at.timestamp() }
}

impl ViewableMedia for MessageMedia {
	fn get(&self) -> CommonMedia {
		CommonMedia {
			source: self.src.as_deref(),
			thumbnail: self.preview.as_deref(),
		}
	}

	fn media_type(&self) -> &MediaType { &self.shared.media_type }
	fn unix_time(&self) -> i64 { self.shared.created_at.timestamp() }
}

impl ViewableMedia for StoryMedia {
	fn get(&self) -> CommonMedia {
		CommonMedia {
			source: self.files.source.url.as_deref(),
			thumbnail: self.files.preview.url.as_deref(),
		}
	}

	fn media_type(&self) -> &MediaType { &self.shared.media_type }
	fn unix_time(&self) -> i64 { self.shared.created_at.timestamp() }
}

impl ViewableMedia for StreamMedia {
	fn get(&self) -> CommonMedia {
		CommonMedia {
			source: None,
			thumbnail: None,
		}
	}

	fn media_type(&self) -> &MediaType { &MediaType::Photo }
	fn unix_time(&self) -> i64 { Utc::now().timestamp() }
}

pub async fn download_media<T: ViewableMedia>(client: &OFClient<Authorized>, media: &[T], path: &Path) {
	join_all(media.iter().filter_map(|media| {
		media.get().source.map(|url| async move {
			client.fetch_file(
				url,
				&path.join(match media.media_type() {
					MediaType::Photo => "Images",
					MediaType::Audio => "Audios",
					MediaType::Video | MediaType::Gif => "Videos",
				}),
				None,
			)
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