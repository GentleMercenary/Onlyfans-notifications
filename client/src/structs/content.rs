#![allow(dead_code)]

use crate::{deserializers::de_str_to_date, client::OFClient, media, user::User};

use std::{slice, fmt};
use futures_util::TryFutureExt;
use reqwest::Url;
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
	#[serde(default)]
	pub text: String,
	pub price: Option<f32>,
	pub author: User,
	#[serde(default)]
	can_toggle_favorite: bool,
	#[serde(default = "Utc::now")]
	#[serde(deserialize_with = "de_str_to_date")]
	posted_at: DateTime<Utc>,
	#[serde(default)]
	media: Vec<media::Feed>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Chat {
	pub id: u64,
	pub text: String,
	pub price: Option<f32>,
	#[serde(default = "Utc::now")]
	#[serde(deserialize_with = "de_str_to_date")]
	created_at: DateTime<Utc>,
	#[serde(default)]
	media: Vec<media::Feed>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Story {
	pub id: u64,
	#[serde(default)]
	can_like: bool,
	#[serde(default = "Utc::now")]
	#[serde(deserialize_with = "de_str_to_date")]
	created_at: DateTime<Utc>,
	#[serde(default)]
	media: Vec<media::Feed>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Notification {
	id: String,
	pub text: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Stream {
	id: u64,
	pub description: String,
	room: String,
	#[serde(default = "Utc::now")]
	#[serde(deserialize_with = "de_str_to_date")]
	started_at: DateTime<Utc>,
	#[serde(flatten)]
	media: media::Stream,
}

pub trait Content {
	fn content_type() -> ContentType;
}

pub trait CanLike: Content {
	fn can_like(&self) -> bool;
	fn like_url(&self) -> Url;
}

pub trait HasMedia: Content {
	type Media: media::Media + Sync + Send;
	fn media(&self) -> &[Self::Media];
}

impl Content for Post {
	fn content_type() -> ContentType { ContentType::Posts }
}

impl CanLike for Post {
	fn can_like(&self) -> bool { self.can_toggle_favorite }
	fn like_url(&self) -> Url {
		Url::parse(&format!("https://onlyfans.com/api2/v2/posts/{}/favorites/{}", self.id, self.author.id)).unwrap()
	}
}

impl HasMedia for Post {
	type Media = media::Feed;
	fn media(&self) -> &[Self::Media] { &self.media }
}

impl Content for Chat {
	fn content_type() -> ContentType { ContentType::Chats }
}

impl CanLike for Chat {
	fn can_like(&self) -> bool { true }
	fn like_url(&self) -> Url {
		Url::parse(&format!("https://onlyfans.com/api2/v2/messages/{}/like", self.id)).unwrap()
	}
}

impl HasMedia for Chat {
	type Media = media::Feed;
	fn media(&self) -> &[Self::Media] { &self.media }
}

impl Content for Story {
	fn content_type() -> ContentType { ContentType::Stories }
}

impl CanLike for Story {
	fn can_like(&self) -> bool { self.can_like }
	fn like_url(&self) -> Url {
		Url::parse(&format!("https://onlyfans.com/api2/v2/stories/{}/like", self.id)).unwrap()
	}
}

impl HasMedia for Story {
	type Media = media::Feed;
	fn media(&self) -> &[Self::Media] { &self.media }
}

impl Content for Notification {
	fn content_type() -> ContentType { ContentType::Notifications }
}

impl Content for Stream {
	fn content_type() -> ContentType { ContentType::Streams }
}

impl HasMedia for Stream {
	type Media = media::Stream;
	fn media(&self) -> &[Self::Media] { slice::from_ref(&self.media) }
}

impl OFClient {
	pub async fn get_post(&self, post_id: u64) -> reqwest::Result<Post> {
		self.get(&format!("https://onlyfans.com/api2/v2/posts/{post_id}"))
		.and_then(|response| response.json::<Post>())
		.await
		.inspect(|content| info!("Got content: {:?}", content))
		.inspect_err(|err| error!("Error reading content {post_id}: {err:?}"))
	}
}