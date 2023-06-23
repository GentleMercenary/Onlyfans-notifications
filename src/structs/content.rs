use crate::deserializers::{de_markdown_string, str_to_date};

use super::User;
use super::media::{MessageMedia, PostMedia, StoryMedia, StreamMedia, ViewableMedia};

use std::slice;
use serde::Deserialize;
use chrono::{DateTime, Utc};
use winrt_toast::{content::text::TextPlacement, Text, Toast};

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PostContent {
	pub id: u64,
	#[serde(deserialize_with = "de_markdown_string")]
	raw_text: String,
	price: Option<f32>,
	pub author: User,
	#[serde(default = "Utc::now")]
	#[serde(deserialize_with = "str_to_date")]
	posted_at: DateTime<Utc>,
	media: Vec<PostMedia>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MessageContent {
	pub id: u64,
	#[serde(deserialize_with = "de_markdown_string")]
	text: String,
	price: Option<f32>,
	#[serde(default = "Utc::now")]
	#[serde(deserialize_with = "str_to_date")]
	posted_at: DateTime<Utc>,
	media: Vec<MessageMedia>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct StoryContent {
	pub id: u64,
	#[serde(default = "Utc::now")]
	#[serde(deserialize_with = "str_to_date")]
	posted_at: DateTime<Utc>,
	media: Vec<StoryMedia>,
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

pub trait Content {
	type Media: ViewableMedia + Sync + Send;

	fn header() -> &'static str;
	fn media(&self) -> Option<&[Self::Media]>;
	fn toast(&self) -> Toast;
}

impl Content for PostContent {
	type Media = PostMedia;

	fn header() -> &'static str { "Posts" }
	fn media(&self) -> Option<&[Self::Media]> { Some(&self.media) }

	fn toast(&self) -> Toast {
		let mut toast = Toast::new();
		toast.text2(&self.raw_text);

		if let Some(price) = self.price && price > 0f32 {
			toast
			.text3(Text::new(format!("${price:.2}"))
			.with_placement(TextPlacement::Attribution));
		}

		toast
	}
}

impl Content for MessageContent {
	type Media = MessageMedia;

	fn header() -> &'static str { "Messages" }
	fn media(&self) -> Option<&[Self::Media]> { Some(&self.media) }

	fn toast(&self) -> Toast {
		let mut toast = Toast::new();
		toast.text2(&self.text);

		if let Some(price) = self.price && price > 0f32 {
			toast
			.text3(Text::new(format!("${price:.2}"))
			.with_placement(TextPlacement::Attribution));
		}

		toast
	}
}

impl Content for StoryContent {
	type Media = StoryMedia;

	fn header() -> &'static str { "Stories" }
	fn media(&self) -> Option<&[Self::Media]> { Some(&self.media) }
	fn toast(&self) -> Toast { Toast::new() }
}

impl Content for NotificationContent {
	type Media = PostMedia;

	fn header() -> &'static str { "Notifications" }
	fn media(&self) -> Option<&[Self::Media]> { None }
	
	fn toast(&self) -> Toast {
		let mut toast = Toast::new();
		toast.text2(&self.text);
		
		toast
	}
}

impl Content for StreamContent {
	type Media = StreamMedia;

	fn header() -> &'static str { "Streams" }
	fn media(&self) -> Option<&[Self::Media]> { Some(slice::from_ref(&self.media)) }

	fn toast(&self) -> Toast {
		let mut toast = Toast::new();

		toast
		.text2(&self.title)
		.text3(&self.description);

		toast
	}
}