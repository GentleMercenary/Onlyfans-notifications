pub mod content;
pub mod media;

use crate::client::{OFClient, Authorized, AuthedClient};
use crate::deserializers::{notification_message, de_markdown_string};
use crate::{MANAGER, SETTINGS, TEMPDIR};

use anyhow::{anyhow, bail};
use content::{ContentType, MessageContent, NotificationContent, PostContent, StoryContent, StreamContent};
use futures::future::{join, join_all};
use media::{ViewableMedia, download_media};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use std::path::Path;
use winrt_toast::{
	content::image::{ImageHintCrop, ImagePlacement},
	Header, Image, Toast,
};

#[derive(Serialize, Debug)]
pub struct ConnectMessage<'a> {
	pub act: &'static str,
	pub token: &'a str,
}

#[derive(Serialize, Debug)]
pub struct GetOnlinesMessage {
	pub act: &'static str,
	pub ids: &'static [&'static u64],
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct InitMessage<'a> {
	#[serde(deserialize_with = "de_markdown_string")]
	pub name: String,
	pub username: &'a str,
	pub ws_auth_token: &'a str,
}

#[derive(Deserialize, Debug)]
pub struct ErrorMessage {
	pub error: u8,
}

#[derive(Deserialize, Debug)]
pub struct ConnectedMessage {
	connected: bool,
	v: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct User {
	id: u64,
	name: String,
	username: String,
	avatar: String,
}

#[derive(Deserialize, Debug)]
pub struct PostPublishedMessage {
	id: String,
	user_id: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
	from_user: User,
	#[serde(flatten)]
	content: MessageContent,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct StoryMessage {
	user_id: u64,
	#[serde(flatten)]
	content: StoryContent,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct NotificationMessage {
	user: User,
	#[serde(rename = "type")]
	notif_type: String,
	sub_type: String,
	#[serde(flatten)]
	content: NotificationContent,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct StreamMessage {
	user: User,
	#[serde(flatten)]
	content: StreamContent,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum TaggedMessageType {
	PostPublished(PostPublishedMessage),
	Api2ChatMessage(ChatMessage),
	Stories(Vec<StoryMessage>),
	Stream(StreamMessage),
}

#[derive(Deserialize, Debug)]
#[serde(untagged)]
pub enum MessageType {
	Tagged(TaggedMessageType),
	Connected(ConnectedMessage),
	#[serde(deserialize_with = "notification_message")]
	NewMessage(NotificationMessage),
	Error(ErrorMessage),
}

fn get_thumbnail<T: ViewableMedia>(media: &[T]) -> Option<&str> {
	media
	.iter()
	.find_map(|media| media.get().thumbnail.filter(|s| !s.is_empty()))
}

async fn handle_content<T: ContentType>(content: &T, client: &OFClient<Authorized>, user: &User) -> anyhow::Result<()> {
	let parsed = user.avatar.parse::<Url>()?;
	let filename = parsed
	.path_segments()
	.and_then(|segments| {
		let mut reverse_iter = segments.rev();
		let ext = reverse_iter.next().and_then(|file| file.split('.').last());
		let filename = reverse_iter.next();

		Option::zip(filename, ext).map(|(filename, ext)| [filename, ext].join("."))
	})
	.ok_or_else(|| anyhow!("Filename unknown"))?;

	let mut user_path = Path::new("data").join(&user.username);
	std::fs::create_dir_all(&user_path)?;
	user_path = user_path.canonicalize()?;

	let (_, avatar) = client.fetch_file(
			&user.avatar,
			&user_path.join("Profile").join("Avatars"),
			Some(&filename),
		)
		.await?;

	let content_type = <T as ContentType>::get_type();
	let mut toast: Toast = content.to_toast();
	toast
	.header(Header::new(content_type, content_type, content_type))
	.launch(user_path.to_str().unwrap())
	.text1(&user.name)
	.image(1,
		Image::new_local(avatar)?
		.with_hint_crop(ImageHintCrop::Circle)
		.with_placement(ImagePlacement::AppLogoOverride),
	);

	if let Some(thumb) = content.get_media().and_then(get_thumbnail) {
		let (_, thumb) = client.fetch_file(thumb, TEMPDIR.wait().path(), None).await?;
		toast.image(2, Image::new_local(thumb)?);
	}

	MANAGER.wait().show(&toast)?;
	Ok(())
}

impl MessageType {
	pub async fn handle_message(self, client: &OFClient<Authorized>) -> anyhow::Result<()> {
		return match self {
			Self::Connected(_) => {
				info!("Connect message received");

				let mut toast = Toast::new();
				toast.text1("OF Notifier").text2("Connection established");

				MANAGER.wait().show(&toast)?;
				Ok(())
			}
			Self::Error(msg) => {
				error!("Error message received: {:?}", msg);
				bail!("websocket received error message with code {}", msg.error)
			}
			Self::NewMessage(msg) => {
				msg.handle(client).await
			}
			Self::Tagged(tagged) => tagged.handle_message(client).await,
		};
	}
}

impl TaggedMessageType {
	async fn handle_message(self, client: &OFClient<Authorized>) -> anyhow::Result<()> {
		match self {
			Self::PostPublished(msg) => msg.handle(client).await,
			Self::Api2ChatMessage(msg) => msg.handle(client).await,
			Self::Stories(msg) => {
				join_all(msg.iter().map(|story| story.handle(client)))
				.await
				.into_iter()
				.find(Result::is_err)
				.unwrap_or(Ok(()))
			}
			Self::Stream(msg) => msg.handle(client).await,
		}
	}
}

async fn shared<T: ContentType + Send + Sync>(user: &User, content: &T, client: &OFClient<Authorized>) -> anyhow::Result<()> {
	let settings = SETTINGS.wait();

	let username = &user.username;
	let notify = handle_content(content, client, user);
	let path = Path::new("data")
		.join(username)
		.join(PostContent::get_type());
	let download = content
		.get_media()
		.map(|media| download_media(client, media, &path));

	if let Some(download) = download && settings.should_download(username) {
		if settings.should_notify(username) {
			return join(notify, download).await.0
		} 

		download.await;
	} else if settings.should_notify(username) {
		return notify.await
	}

	Ok(())
}

impl PostPublishedMessage {
	pub async fn handle(&self, client: &OFClient<Authorized>) -> anyhow::Result<()> {
		info!("Post message received: {:?}", self);

		let content = client.fetch_post(&self.id).await?;
		shared(&content.author, &content, client).await
	}
}

impl ChatMessage {
	pub async fn handle(&self, client: &OFClient<Authorized>) -> anyhow::Result<()> {
		info!("Chat message received: {:?}", self);
		shared(&self.from_user, &self.content, client).await
	}
}

impl StoryMessage {
	pub async fn handle(&self, client: &OFClient<Authorized>) -> anyhow::Result<()> {
		info!("Story message received: {:?}", self);

		let user = client.fetch_user(&self.user_id).await?;
		shared(&user, &self.content, client).await
	}
}

impl NotificationMessage {
	pub async fn handle(&self, client: &OFClient<Authorized>) -> anyhow::Result<()> {
		info!("Notification message received: {:?}", self);
		shared(&self.user, &self.content, client).await
	}
}

impl StreamMessage {
	pub async fn handle(&self, client: &OFClient<Authorized>) -> anyhow::Result<()> {
		info!("Stream message received: {:?}", self);
		shared(&self.user, &self.content, client).await
	}
}
