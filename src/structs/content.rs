use crate::{deserializers::{de_markdown_string, str_to_date}, client::{OFClient, Authorized}};

use super::{media, user::User};

use std::{slice, fmt};
use futures_util::TryFutureExt;
use serde::Deserialize;
use chrono::{DateTime, Utc};
use winrt_toast::{content::text::TextPlacement, Text, Toast};

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Post {
	pub id: u64,
	#[serde(deserialize_with = "de_markdown_string")]
	raw_text: String,
	price: Option<f32>,
	pub author: User,
	#[serde(default = "Utc::now")]
	#[serde(deserialize_with = "str_to_date")]
	posted_at: DateTime<Utc>,
	media: Vec<media::Post>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Message {
	pub id: u64,
	#[serde(deserialize_with = "de_markdown_string")]
	text: String,
	price: Option<f32>,
	#[serde(default = "Utc::now")]
	#[serde(deserialize_with = "str_to_date")]
	posted_at: DateTime<Utc>,
	media: Vec<media::Message>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Story {
	pub id: u64,
	#[serde(default = "Utc::now")]
	#[serde(deserialize_with = "str_to_date")]
	posted_at: DateTime<Utc>,
	media: Vec<media::Story>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Notification {
	id: String,
	#[serde(deserialize_with = "de_markdown_string")]
	text: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Stream {
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
	media: media::Stream,
}

pub trait Content {
	type Media: media::Media + Sync + Send;

	fn header() -> &'static str;
	fn media(&self) -> Option<&[Self::Media]>;
	fn toast(&self) -> Toast;
}

impl Content for Post {
	type Media = media::Post;

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

impl Content for Message {
	type Media = media::Message;

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

impl Content for Story {
	type Media = media::Story;

	fn header() -> &'static str { "Stories" }
	fn media(&self) -> Option<&[Self::Media]> { Some(&self.media) }
	fn toast(&self) -> Toast { Toast::new() }
}

impl Content for Notification {
	type Media = media::Post;

	fn header() -> &'static str { "Notifications" }
	fn media(&self) -> Option<&[Self::Media]> { None }
	
	fn toast(&self) -> Toast {
		let mut toast = Toast::new();
		toast.text2(&self.text);
		
		toast
	}
}

impl Content for Stream {
	type Media = media::Stream;

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

impl OFClient<Authorized> {
	pub async fn get_post<I: fmt::Display>(&self, post_id: I) -> anyhow::Result<Post> {
		self.get(&format!("https://onlyfans.com/api2/v2/posts/{post_id}"))
		.and_then(|response| response.json::<Post>().map_err(Into::into))
		.await
		.inspect(|content| info!("Got content: {:?}", content))
		.inspect_err(|err| error!("Error reading content {post_id}: {err:?}"))
	}

	pub async fn like_post(&self, post: &Post) -> anyhow::Result<()> {
		let user_id = post.author.id;
		let post_id = post.id;

		self.post(&format!("https://onlyfans.com/api2/v2/posts/{post_id}/favorites/{user_id}"), None as Option<&String>)
		.await
		.map(|_| ())
	}
	
	pub async fn like_message(&self, message: &Message) -> anyhow::Result<()> {
		let message_id = message.id;

		self.post(&format!("https://onlyfans.com/api2/v2/messages/{message_id}/like"), None as Option<&String>)
		.await
		.map(|_| ())
	}

	pub async fn like_story(&self, story: &Story) -> anyhow::Result<()> {
		let story_id = story.id;

		self.post(&format!("https://onlyfans.com/api2/v2/stories/{story_id}/like"), None as Option<&String>)
		.await
		.map(|_| ())
	}
}