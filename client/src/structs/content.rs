use crate::{deserializers::{de_markdown_string, str_to_date}, client::{OFClient, Authorized}, media, user::User};

use std::{slice, fmt};
use futures_util::TryFutureExt;
use reqwest::Response;
use serde::Deserialize;
use chrono::{DateTime, Utc};

pub enum ContentType {
    Posts,
    Chats,
    Stories,
    Notifications,
    Streams
}

impl fmt::Display for ContentType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str( match self {
            ContentType::Posts => "Posts",
            ContentType::Chats => "Messages",
            ContentType::Stories => "Stories",
            ContentType::Notifications => "Notifications",
            ContentType::Streams => "Streams",
        })
    }
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Post {
	pub id: u64,
	#[serde(deserialize_with = "de_markdown_string")]
	pub raw_text: String,
	pub price: Option<f32>,
	pub author: User,
	#[serde(default = "Utc::now")]
	#[serde(deserialize_with = "str_to_date")]
	posted_at: DateTime<Utc>,
	media: Vec<media::Post>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Chat {
	pub id: u64,
	#[serde(deserialize_with = "de_markdown_string")]
	pub text: String,
	pub price: Option<f32>,
	#[serde(default = "Utc::now")]
	#[serde(deserialize_with = "str_to_date")]
	posted_at: DateTime<Utc>,
	media: Vec<media::Chat>,
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
	pub text: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Stream {
	id: u64,
	#[serde(deserialize_with = "de_markdown_string")]
	pub description: String,
	#[serde(deserialize_with = "de_markdown_string")]
	pub title: String,
	room: String,
	#[serde(default = "Utc::now")]
	#[serde(deserialize_with = "str_to_date")]
	started_at: DateTime<Utc>,
	#[serde(flatten)]
	media: media::Stream,
}

pub trait Content {
	type Media: media::Media + Sync + Send;

	fn media(&self) -> Option<&[Self::Media]>;
	fn content_type() -> ContentType;
}

impl Content for Post {
	type Media = media::Post;

	fn media(&self) -> Option<&[Self::Media]> { Some(&self.media) }
	fn content_type() -> ContentType { ContentType::Posts }
}

impl Content for Chat {
	type Media = media::Chat;
	
	fn media(&self) -> Option<&[Self::Media]> { Some(&self.media) }
	fn content_type() -> ContentType { ContentType::Chats }
}

impl Content for Story {
	type Media = media::Story;
	
	fn media(&self) -> Option<&[Self::Media]> { Some(&self.media) }
	fn content_type() -> ContentType { ContentType::Stories }
}

impl Content for Notification {
	type Media = media::Post;
	
	fn media(&self) -> Option<&[Self::Media]> { None }
	fn content_type() -> ContentType { ContentType::Notifications }
}

impl Content for Stream {
	type Media = media::Stream;
	
	fn media(&self) -> Option<&[Self::Media]> { Some(slice::from_ref(&self.media)) }
	fn content_type() -> ContentType { ContentType::Streams }
}

impl OFClient<Authorized> {
	pub async fn get_post(&self, post_id: u64) -> reqwest::Result<Post> {
		self.get(&format!("https://onlyfans.com/api2/v2/posts/{post_id}"))
		.and_then(|response| response.json::<Post>())
		.await
		.inspect(|content| info!("Got content: {:?}", content))
		.inspect_err(|err| error!("Error reading content {post_id}: {err:?}"))
	}

	pub async fn like_post(&self, post: &Post) -> reqwest::Result<Response> {
		let user_id = post.author.id;
		let post_id = post.id;

		self.post(&format!("https://onlyfans.com/api2/v2/posts/{post_id}/favorites/{user_id}"), None as Option<&String>)
		.await
	}
	
	pub async fn like_chat(&self, chat: &Chat) -> reqwest::Result<Response> {
		let chat_id = chat.id;

		self.post(&format!("https://onlyfans.com/api2/v2/messages/{chat_id}/like"), None as Option<&String>)
		.await
	}

	pub async fn like_story(&self, story: &Story) -> reqwest::Result<Response> {
		let story_id = story.id;

		self.post(&format!("https://onlyfans.com/api2/v2/stories/{story_id}/like"), None as Option<&String>)
		.await
	}
}