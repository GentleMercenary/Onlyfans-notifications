use crate::{MANAGER, SETTINGS, deserializers::{notification_message, from_string}, settings::{Settings, ShouldNotify, ShouldDownload, ShouldLike}};

use of_client::{user::User, content::{self, CanLike, HasMedia}, client::OFClient, media::{Media, MediaType}};
use anyhow::bail;
use serde::{Deserialize, Serialize};
use futures::future::{join, join_all};
use filetime::{FileTime, set_file_mtime};
use futures_util::future::join3;
use std::path::Path;
use winrt_toast::{Toast, Text, content::text::TextPlacement, Header};

use super::{ToastExt, fetch_file};

#[derive(Serialize, Debug)]
pub struct Connect<'a> {
	pub act: &'static str,
	pub token: &'a str,
}

#[derive(Serialize, Debug)]
pub struct Heartbeat<'a> {
	pub act: &'static str,
	pub ids: &'a [u64],
}

#[derive(Deserialize, Debug)]
pub struct Onlines {
	online: Vec<u64>
}

#[derive(Deserialize, Debug)]
pub struct Error {
	pub error: u8,
}

#[derive(Deserialize, Debug)]
pub struct Connected {
	connected: bool,
	v: String,
}

#[derive(Deserialize, Debug)]
pub struct PostPublished {
	#[serde(deserialize_with = "from_string")]
	id: u64,
	#[serde(deserialize_with="from_string")]
	user_id: u64,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Chat {
	from_user: User,
	#[serde(flatten)]
	content: content::Chat,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Story {
	user_id: u64,
	#[serde(flatten)]
	content: content::Story,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Notification {
	user: User,
	#[serde(rename = "type")]
	notif_type: String,
	sub_type: String,
	#[serde(flatten)]
	content: content::Notification,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Stream {
	user: User,
	#[serde(flatten)]
	content: content::Stream,
}

pub trait ToToast: content::Content {
	fn to_toast(&self) -> Toast;
}

impl ToToast for content::Post {
    fn to_toast(&self) -> Toast {
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

impl ToToast for content::Chat {
    fn to_toast(&self) -> Toast {
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

impl ToToast for content::Story {
    fn to_toast(&self) -> Toast {
        Toast::new()
    }
}

impl ToToast for content::Notification {
    fn to_toast(&self) -> Toast {
        let mut toast = Toast::new();
		toast.text2(&self.text);
		
		toast
    }
}

impl ToToast for content::Stream {
    fn to_toast(&self) -> Toast {
        let mut toast = Toast::new();

		toast
		.text2(&self.description);

		toast
    }
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum TaggedMessage {
	PostPublished(PostPublished),
	Api2ChatMessage(Chat),
	Stories(Vec<Story>),
	Stream(Stream),
}

#[derive(Deserialize, Debug)]
#[serde(untagged)]
pub enum Message {
	Tagged(TaggedMessage),
	Connected(Connected),
	Onlines(Onlines),
	#[serde(deserialize_with = "notification_message")]
	NewMessage(Notification),
	Error(Error),
}

impl Message {
	pub async fn handle_message(self, client: &OFClient) -> anyhow::Result<()> {
		match self {
			Self::Connected(_) | Self::Onlines(_) => Ok(()),
			Self::Error(msg) => {
				error!("Error message received: {:?}", msg);
				bail!("websocket received error message with code {}", msg.error)
			},
			_ => {
				let settings = &SETTINGS.get().unwrap().read().await;
				match self {
					Self::NewMessage(msg) => {
						info!("Notification message received: {:?}", msg);
						notify(&msg.content, &msg.user, client, settings).await
					},
					Self::Tagged(TaggedMessage::Stream(msg)) => {
						info!("Stream message received: {:?}", msg);
						join(
							notify_with_thumbnail(&msg.content, &msg.user, client, settings),
							download(&msg.content, &msg.user, client, settings)
						).await.0
					},
					Self::Tagged(TaggedMessage::PostPublished(msg)) => {
						info!("Post message received: {:?}", msg);
						let content = client.get_post(msg.id).await?;

						join3(
							notify_with_thumbnail(&content, &content.author, client, settings),
							download(&content, &content.author, client, settings),
							like(&content, &content.author, client, settings)
						).await.0
					},
					Self::Tagged(TaggedMessage::Api2ChatMessage(msg)) => {
						info!("Chat message received: {:?}", msg);
						join3(
							notify_with_thumbnail(&msg.content, &msg.from_user, client, settings),
							download(&msg.content, &msg.from_user, client, settings),
							like(&msg.content, &msg.from_user, client, settings)
						).await.0
					},
					Self::Tagged(TaggedMessage::Stories(msg)) => {
						info!("Story message received: {:?}", msg);
						join_all(msg.iter().map(|story| async move {
							let user = client.get_user(story.user_id).await?;

							join3(
								notify_with_thumbnail(&story.content, &user, client, settings),
								download(&story.content, &user, client, settings),
								like(&story.content, &user, client, settings)
							).await.0
						}))
						.await
						.into_iter()
						.find(Result::is_err)
						.unwrap_or(Ok(()))
					},
					_ => unreachable!()
				}
			}
		}
	}
}

async fn notify<T: ToToast>(content: &T, user: &User, client: &OFClient, settings: &Settings) -> anyhow::Result<()> {	
	if content.should_notify(&user.username, settings) {
		let header = T::content_type().to_string();
		let mut toast = content.to_toast();

		toast
		.header(Header::new(&header, &header, ""))
		.text1(&user.name)
		.with_avatar(user, client).await?;

		MANAGER.get().unwrap().show(&toast)?;
	}
	Ok(())
}

async fn notify_with_thumbnail<T: ToToast + HasMedia>(content: &T, user: &User, client: &OFClient, settings: &Settings) -> anyhow::Result<()> {
	if content.should_notify(&user.username, settings) {
		let header = T::content_type().to_string();
		let mut toast = content.to_toast();

		toast
		.header(Header::new(&header, &header, ""))
		.text1(&user.name)
		.with_avatar(user, client).await?
		.with_thumbnail(content.media(), client).await?;

		MANAGER.get().unwrap().show(&toast)?;
	}
	Ok(())
}

async fn download<T: ToToast + HasMedia>(content: &T, user: &User, client: &OFClient, settings: &Settings) {
	if content.should_download(&user.username, settings) {
		let header = T::content_type().to_string();
		let content_path = Path::new("data").join(&user.username).join(&header);

		let _ = join_all(content.media().iter().filter_map(|media| {
			let path = content_path.join(match media.media_type() {
				MediaType::Photo => "Images",
				MediaType::Audio => "Audios",
				MediaType::Video | MediaType::Gif => "Videos",
			});
	
			media.source().map(|url| async move {
				fetch_file(client, url, &path, None)
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
}

async fn like<T: ToToast + CanLike>(content: &T, user: &User, client: &OFClient, settings: &Settings) {
	if content.can_like() && content.should_like(&user.username, settings) {
		let _ = client.post(content.like_url(), None as Option<&String>).await;
	}
}