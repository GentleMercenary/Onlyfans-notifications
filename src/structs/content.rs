use super::User;
use super::media::{MessageMedia, PostMedia, StoryMedia, StreamMedia, ViewableMedia};
use crate::deserializers::{de_markdown_string, str_to_date};

use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::slice;
use winrt_toast::{content::text::TextPlacement, Text, Toast};

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Content<T: ViewableMedia> {
	pub id: u64,
	#[serde(default = "Utc::now")]
	#[serde(deserialize_with = "str_to_date")]
	posted_at: DateTime<Utc>,
	media: Vec<T>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PostContent {
	#[serde(deserialize_with = "de_markdown_string")]
	raw_text: String,
	price: Option<f32>,
	pub author: User,
	#[serde(flatten)]
	pub shared: Content<PostMedia>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MessageContent {
	#[serde(deserialize_with = "de_markdown_string")]
	text: String,
	price: Option<f32>,
	#[serde(flatten)]
	shared: Content<MessageMedia>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct StoryContent {
	#[serde(flatten)]
	shared: Content<StoryMedia>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct NotificationContent {
	id: String,
	#[serde(deserialize_with = "de_markdown_string")]
	text: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct StreamContent {
	id: u64,
	#[serde(deserialize_with = "de_markdown_string")]
	description: String,
	#[serde(deserialize_with = "de_markdown_string")]
	title: String,
	room: String,
	#[serde(default = "Utc::now")]
	#[serde(deserialize_with = "str_to_date")]
	started_at: DateTime<Utc>,
	#[serde(flatten)]
	media: StreamMedia,
}

pub trait ContentType {
	type Media: ViewableMedia + Sync + Send;

	fn get_type() -> &'static str;
	fn get_media(&self) -> Option<&[Self::Media]>;
	fn to_toast(&self) -> Toast;
}

impl ContentType for PostContent {
	type Media = PostMedia;

	fn get_type() -> &'static str { "Posts" }

	fn to_toast(&self) -> Toast {
		let mut toast = Toast::new();
		toast.text2(&self.raw_text);

		if let Some(price) = self.price && price > 0f32 {
			toast.text3(Text::new(format!("${price:.2}"))
				.with_placement(TextPlacement::Attribution));
		}

		toast
	}

	fn get_media(&self) -> Option<&[Self::Media]> { Some(&self.shared.media) }
}

impl ContentType for MessageContent {
	type Media = MessageMedia;

	fn get_type() -> &'static str { "Messages" }

	fn to_toast(&self) -> Toast {
		let mut toast = Toast::new();
		toast.text2(&self.text);

		if let Some(price) = self.price && price > 0f32 {
			toast.text3(Text::new(format!("${price:.2}"))
				.with_placement(TextPlacement::Attribution));
		}

		toast
	}

	fn get_media(&self) -> Option<&[Self::Media]> { Some(&self.shared.media) }
}

impl ContentType for StoryContent {
	type Media = StoryMedia;

	fn get_type() -> &'static str { "Stories" }

	fn to_toast(&self) -> Toast {
		Toast::new()
	}

	fn get_media(&self) -> Option<&[Self::Media]> { Some(&self.shared.media) }
}

impl ContentType for NotificationContent {
	type Media = PostMedia;

	fn get_type() -> &'static str { "Notification" }
	
	fn to_toast(&self) -> Toast {
		let mut toast = Toast::new();
		toast.text2(&self.text);
		
		toast
	}

	fn get_media(&self) -> Option<&[Self::Media]> { None }
}

impl ContentType for StreamContent {
	type Media = StreamMedia;

	fn get_type() -> &'static str { "Stream" }

	fn to_toast(&self) -> Toast {
		let mut toast = Toast::new();
		
		if !self.title.is_empty() {
			toast.text2(&self.title);
		}
		
		if !self.description.is_empty() {
			toast.text3(&self.description);
		}

		toast
	}

	fn get_media(&self) -> Option<&[Self::Media]> { Some(slice::from_ref(&self.media)) }
}
