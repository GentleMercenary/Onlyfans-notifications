#![allow(dead_code)]

use crate::{MANAGER, SETTINGS};

use super::client::ClientExt;
use super::deserializers::*;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use filetime::{set_file_mtime, FileTime};
use futures::{future::{join_all, join, try_join}, Future};
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use std::{error, path::Path, process::Command};
use winrt_toast::{
	content::{
		image::{ImageHintCrop, ImagePlacement},
		text::TextPlacement,
	},
	Header, Image, Text, Toast,
};

pub type Error = Box<dyn error::Error + Send + Sync>;

#[derive(Serialize, Debug)]
pub struct ConnectMessage<'a> {
	pub act: &'static str,
	pub token: &'a str,
}

#[derive(Serialize, Debug)]
pub struct GetOnlinesMessage {
	pub act: &'static str,
	pub ids: &'static [&'static i32],
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct InitMessage<'a> {
	pub username: &'a str,
	pub ws_auth_token: &'a str,
}

#[derive(Deserialize, Debug, Clone)]
pub struct ErrorMessage {
	error: u8,
}

#[derive(Deserialize, Debug)]
pub struct ConnectedMessage {
	connected: bool,
	v: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Content<T: ContentType> {
	#[serde(default = "Utc::now")]
	#[serde(deserialize_with = "str_to_date")]
	posted_at: DateTime<Utc>,
	#[serde(flatten)]
	unique: T
}

async fn handle_content<T: ContentType>(content: &Content<T>, client: &Client, user: &User) -> Result<(), Error> {
	let parsed = user.avatar.parse::<Url>()?;
	let filename = parsed
		.path_segments()
		.and_then(|segments| {
			let mut reverse_iter = segments.rev();
			let ext = reverse_iter.next().and_then(|file| file.split('.').last());
			let filename = reverse_iter.next();

			Option::zip(filename, ext).map(|(filename, ext)| [filename, ext].join("."))
		})
		.ok_or("Filename unknown")?;

	let mut user_path = Path::new("data").join(&user.username);
	std::fs::create_dir_all(&user_path)?;
	user_path = user_path.canonicalize()?;

	let avatar = client
		.fetch_file(
			&user.avatar,
			&user_path.join("Profile").join("Avatars"),
			Some(&filename),
		)
		.await?;

	info!("Creating notification");
	let content_type = <T as ContentType>::get_type();
	let mut toast: Toast = <T as ContentType>::to_toast(&content.unique);
	toast
		.header(Header::new(content_type, content_type, content_type))
		.launch(user_path.join(content_type).to_str().unwrap())
		.text1(&user.name).image(
			1,
			Image::new_local(avatar)?
				.with_hint_crop(ImageHintCrop::Circle)
				.with_placement(ImagePlacement::AppLogoOverride),
		);

	if let Some(thumb) = <T as ContentType>::get_thumbnail(&content.unique) {
		let thumb = client
			.fetch_file(&thumb, &user_path.join("thumbs"), None)
			.await?;
		toast.image(2, Image::new_local(thumb)?);
	}

	MANAGER
		.wait()
		.show_with_callbacks(
			&toast,
			Some(Box::new(move |rs| {
				if let Ok(s) = rs {
					Command::new("explorer").arg(s).spawn().unwrap();
				}
			})),
			None,
			Some(Box::new(move |e| {
				error!("Could't show notification: {:?}", e);
			})),
		)
		.inspect_err(|err| error!("{err}"))
		.map_err(|err| err.into())
}

pub trait ContentType {
	fn get_type() -> &'static str;
	fn get_thumbnail(&self) -> Option<&str>;
	fn to_toast(&self) -> Toast;
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PostContent {
	#[serde(deserialize_with = "de_markdown_string")]
	text: String,
	price: Option<f32>,
	media: Vec<Media<PostMedia>>
}

impl ContentType for PostContent {
	fn get_type() -> &'static str {
		"Posts"
	}
	
	fn get_thumbnail(&self) -> Option<&str> {
		self
		.media
		.iter()
		.find_map(|media| media.unique.preview.as_deref().filter(|s| s != &""))
	}

	fn to_toast(&self) -> Toast {
		let mut toast = Toast::new();

		info!("{}", &self.text);
		toast.text2(&self.text);

		if let Some(price) = self.price && price > 0f32 {
			toast.text3(Text::new(format!("${:.2}", price))
				.with_placement(TextPlacement::Attribution));
		}

		toast
	}
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MessageContent {
	#[serde(deserialize_with = "de_markdown_string")]
	text: String,
	price: Option<f32>,
	media: Vec<Media<MessageMedia>>
}

impl ContentType for MessageContent {
	fn get_type() -> &'static str {
		"Messages"
	}

	fn get_thumbnail(&self) -> Option<&str> {
		self
		.media
		.iter()
		.find_map(|media| media.unique.preview.as_deref().filter(|s| s != &""))
	}

	fn to_toast(&self) -> Toast {
		let mut toast = Toast::new();

		info!("{}", &self.text);
		toast.text2(&self.text);

		if let Some(price) = self.price && price > 0f32 {
			toast.text3(Text::new(format!("${:.2}", price))
				.with_placement(TextPlacement::Attribution));
		}

		toast
	}
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct StoryContent {
	media: Vec<Media<StoryMedia>>
}

impl ContentType for StoryContent {
	fn get_type() -> &'static str {
		"Stories"
	}

	fn get_thumbnail(&self) -> Option<&str> {
		self
		.media
		.iter()
		.find_map(|media| media.unique.files.preview.url.as_deref().filter(|s| s != &""))
	}

	fn to_toast(&self) -> Toast {
		Toast::new()
	}
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum MediaTypes {
	Photo,
	Video,
	Gif,
	Audio,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Media<T: ViewableMedia> {
	id: u64,
	can_view: bool,
	#[serde(default = "Utc::now")]
	#[serde(deserialize_with = "str_to_date")]
	created_at: DateTime<Utc>,
	#[serde(flatten)]
	unique: T,
	#[serde(alias = "type")]
	media_type: MediaTypes,
}

pub struct _MediaInner<'a> {
	source: &'a Option<String>,
	thumbnail: &'a Option<String>
}

pub trait ViewableMedia {
	fn get(&self) -> _MediaInner;
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PostMedia {
	full: Option<String>,
	preview: Option<String>,
}

impl ViewableMedia for PostMedia {
	fn get(&self) -> _MediaInner {
		_MediaInner {
			source: &self.full,
			thumbnail: &self.preview
		}
	}
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MessageMedia {
	src: Option<String>,
	preview: Option<String>
}

impl ViewableMedia for MessageMedia {
	fn get(&self) -> _MediaInner {
		_MediaInner {
			source: &self.src,
			thumbnail: &self.preview
		}
	}
}

#[derive(Deserialize, Debug)]
struct _FilesInner {
	url: Option<String>
}

#[derive(Deserialize, Debug)]
struct _Files {
	source: _FilesInner,
	preview: _FilesInner
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct StoryMedia {
	files: _Files
}

impl ViewableMedia for StoryMedia {
	fn get(&self) -> _MediaInner {
		_MediaInner {
			source: &self.files.source.url,
			thumbnail: &self.files.preview.url
		}
	}
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct User {
	id: u32,
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
	content: Content<MessageContent>

}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct StoryMessage {
	user_id: u32,
	#[serde(flatten)]
	content: Content<StoryContent>
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum TaggedMessageType {
	PostPublished(PostPublishedMessage),
	Api2ChatMessage(ChatMessage),
	Stories(Vec<StoryMessage>),
}

#[derive(Deserialize, Debug)]
#[serde(untagged)]
pub enum MessageType {
	Tagged(TaggedMessageType),
	Connected(ConnectedMessage),
	Error(ErrorMessage),
}

async fn download_media<T: ViewableMedia>(client: &Client, media: &Vec<Media<T>>, path: &Path) {
	join_all(media.iter().filter_map(|media| {
		media.unique.get().source.as_ref().map(|url| async move {
			client
				.fetch_file(
					&url,
					&path.join(match media.media_type {
						MediaTypes::Photo => "Images",
						MediaTypes::Audio => "Audios",
						MediaTypes::Video | MediaTypes::Gif => "Videos",
					}),
					None,
				)
				.await
				.inspect_err(|err| error!("Download failed: {err}"))
				.and_then(|path| {
					set_file_mtime(
							path,
							FileTime::from_unix_time(media.created_at.timestamp(), 0),
						)
						.inspect_err(|err| error!("{err}"))
						.map_err(|err| err.into())
				})
		})
	}))
	.await;
}

#[async_trait]
pub trait Handleable {
	async fn handle_message(self) -> Result<(), Error>;
}

#[async_trait]
impl Handleable for MessageType {
	async fn handle_message(self) -> Result<(), Error> {
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
				Err(format!("websocket received error message with code {}", msg.error).into())
			}
			Self::Tagged(tagged) => {
				tagged.handle_message().await
			}
		};
	}
}

#[async_trait]
impl Handleable for TaggedMessageType {
	async fn handle_message(self) -> Result<(), Error> {
		let client = Client::with_auth().await?;

		match self {
			TaggedMessageType::PostPublished(msg) => {
				msg.handle(&client).await
			},
			TaggedMessageType::Api2ChatMessage(msg) => {
				msg.handle(&client).await
			},
			TaggedMessageType::Stories(msg) => {
				join_all(msg.iter().map(|story| story.handle(&client) ))
				.await.into_iter().find(|res| res.is_err()).unwrap_or(Ok(()))
			}
		}
	}
}

#[async_trait]
pub trait Message {
	async fn handle(&self, client: &Client) -> Result<(), Error>;
	async fn shared(username: &str, notify: impl Future<Output = Result<(), Error>> + Send, download: impl Future<Output = ()> + Send) -> Result <(), Error> {
		let settings = SETTINGS.wait();
		if settings.should_download(username) {
			if settings.should_notify(username) {
				return join(notify, download).await.0
			} else {
				download.await;
			}
		} else if settings.should_notify(username) {
			return notify.await;
		}
		
		Ok(())
	}
}

#[async_trait]
impl Message for PostPublishedMessage {
	async fn handle(&self, client: &Client) -> Result<(), Error> {
		info!("Post message received: {:?}", self);

		let (user, content) = try_join(
			client.fetch_user(&self.user_id),
			client.fetch_content(&self.id)
		).await?;

		let username = &user.username;
		let notify = handle_content(&content, &client, &user);
		let path = Path::new("data").join(&username).join(PostContent::get_type());
		let download = download_media(
					&client,
					&content.unique.media,
					&path,
				);

		Self::shared(&username, notify, download).await

	}
}

#[async_trait]
impl Message for ChatMessage {
	async fn handle(&self, client: &Client) -> Result<(), Error> {
		info!("Chat message received: {:?}", self);

		let username = &self.from_user.username;
		let notify = handle_content(&self.content, &client, &self.from_user);
		let path = Path::new("data").join(&username).join(MessageContent::get_type());
		let download = download_media(
					&client,
					&self.content.unique.media,
					&path,
				);

		Self::shared(&username, notify, download).await
	}
}

#[async_trait]
impl Message for StoryMessage {
	async fn handle(&self, client: &Client) -> Result<(), Error> {
		info!("Story message received: {:?}", self);

		let user = client.fetch_user(&self.user_id.to_string()).await?;
		let username = &user.username;
		let notify = handle_content(&self.content, &client, &user);
		let path = Path::new("data").join(&username).join(StoryContent::get_type());
		let download = download_media(
					&client,
					&self.content.unique.media,
					&path,
				);

		Self::shared(&username, notify, download).await
	}
}