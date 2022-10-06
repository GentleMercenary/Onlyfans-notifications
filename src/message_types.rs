#![allow(dead_code)]

use crate::{MANAGER, SETTINGS, TEMPDIR};

use super::client::ClientExt;
use super::deserializers::*;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use filetime::{set_file_mtime, FileTime};
use futures::future::{join, join_all, try_join};
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use std::{error, path::Path};
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
pub struct Content<T: ViewableMedia> {
	id: u64,
	#[serde(default = "Utc::now")]
	#[serde(deserialize_with = "str_to_date")]
	posted_at: DateTime<Utc>,
	media: Vec<T>,
}

pub trait ContentType {
	type Media: ViewableMedia + Sync + Send;

	fn get_type() -> &'static str;
	fn get_media(&self) -> &Vec<Self::Media>;
	fn to_toast(&self) -> Toast;
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PostContent {
	#[serde(deserialize_with = "de_markdown_string")]
	text: String,
	price: Option<f32>,
	#[serde(flatten)]
	shared: Content<PostMedia>,
}

impl ContentType for PostContent {
	type Media = PostMedia;

	fn get_type() -> &'static str {
		"Posts"
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

	fn get_media(&self) -> &Vec<Self::Media> {
		&self.shared.media
	}
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

impl ContentType for MessageContent {
	type Media = MessageMedia;

	fn get_type() -> &'static str {
		"Messages"
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

	fn get_media(&self) -> &Vec<Self::Media> {
		&self.shared.media
	}
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct StoryContent {
	#[serde(flatten)]
	shared: Content<StoryMedia>,
}

impl ContentType for StoryContent {
	type Media = StoryMedia;

	fn get_type() -> &'static str {
		"Stories"
	}

	fn to_toast(&self) -> Toast {
		Toast::new()
	}

	fn get_media(&self) -> &Vec<Self::Media> {
		&self.shared.media
	}
}

fn get_thumbnail<T: ViewableMedia>(media: &[T]) -> Option<&str> {
	media
		.iter()
		.find_map(|media| media.get().thumbnail.as_deref().filter(|s| s != &""))
}

async fn handle_content<T: ContentType>(
	content: &T,
	client: &Client,
	user: &User,
) -> Result<(), Error> {
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
	let mut toast: Toast = content.to_toast();
	toast
		.header(Header::new(content_type, content_type, content_type))
		.launch(user_path.to_str().unwrap())
		.text1(&user.name)
		.image(
			1,
			Image::new_local(avatar)?
				.with_hint_crop(ImageHintCrop::Circle)
				.with_placement(ImagePlacement::AppLogoOverride),
		);

	if let Some(thumb) = get_thumbnail(content.get_media()) {
		let thumb = client
			.fetch_file(thumb, TEMPDIR.wait().path(), None)
			.await?;
		toast.image(2, Image::new_local(thumb)?);
	}

	MANAGER.wait().show(&toast).map_err(|err| err.into())
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
pub struct Media {
	id: u64,
	can_view: bool,
	#[serde(default = "Utc::now")]
	#[serde(deserialize_with = "str_to_date")]
	created_at: DateTime<Utc>,
	#[serde(rename = "type")]
	media_type: MediaTypes,
}

pub struct _MediaInner<'a> {
	source: &'a Option<String>,
	thumbnail: &'a Option<String>,
}

pub trait ViewableMedia {
	fn get(&self) -> _MediaInner;
	fn media_type(&self) -> &MediaTypes;
	fn unix_time(&self) -> i64;
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PostMedia {
	full: Option<String>,
	preview: Option<String>,
	#[serde(flatten)]
	shared: Media,
}

impl ViewableMedia for PostMedia {
	fn get(&self) -> _MediaInner {
		_MediaInner {
			source: &self.full,
			thumbnail: &self.preview,
		}
	}

	fn media_type(&self) -> &MediaTypes {
		&self.shared.media_type
	}

	fn unix_time(&self) -> i64 {
		self.shared.created_at.timestamp()
	}
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MessageMedia {
	src: Option<String>,
	preview: Option<String>,
	#[serde(flatten)]
	shared: Media,
}

impl ViewableMedia for MessageMedia {
	fn get(&self) -> _MediaInner {
		_MediaInner {
			source: &self.src,
			thumbnail: &self.preview,
		}
	}

	fn media_type(&self) -> &MediaTypes {
		&self.shared.media_type
	}

	fn unix_time(&self) -> i64 {
		self.shared.created_at.timestamp()
	}
}

#[derive(Deserialize, Debug)]
struct _FilesInner {
	url: Option<String>,
}

#[derive(Deserialize, Debug)]
struct _Files {
	source: _FilesInner,
	preview: _FilesInner,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct StoryMedia {
	files: _Files,
	#[serde(flatten)]
	shared: Media,
}

impl ViewableMedia for StoryMedia {
	fn get(&self) -> _MediaInner {
		_MediaInner {
			source: &self.files.source.url,
			thumbnail: &self.files.preview.url,
		}
	}

	fn media_type(&self) -> &MediaTypes {
		&self.shared.media_type
	}

	fn unix_time(&self) -> i64 {
		self.shared.created_at.timestamp()
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
	content: MessageContent,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct StoryMessage {
	user_id: u32,
	#[serde(flatten)]
	content: StoryContent,
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

async fn download_media<T: ViewableMedia>(client: &Client, media: &[T], path: &Path) {
	join_all(media.iter().filter_map(|media| {
		media.get().source.as_ref().map(|url| async move {
			client
				.fetch_file(
					url,
					&path.join(match media.media_type() {
						MediaTypes::Photo => "Images",
						MediaTypes::Audio => "Audios",
						MediaTypes::Video | MediaTypes::Gif => "Videos",
					}),
					None,
				)
				.await
				.inspect_err(|err| error!("Download failed: {err}"))
				.inspect(|path| {
					let _ = set_file_mtime(path, FileTime::from_unix_time(media.unix_time(), 0))
						.inspect_err(|err| warn!("Error setting file modify time: {err}"));
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
			Self::Tagged(tagged) => tagged.handle_message().await,
		};
	}
}

#[async_trait]
impl Handleable for TaggedMessageType {
	async fn handle_message(self) -> Result<(), Error> {
		let client = Client::with_auth().await?;

		match self {
			TaggedMessageType::PostPublished(msg) => msg.handle(&client).await,
			TaggedMessageType::Api2ChatMessage(msg) => msg.handle(&client).await,
			TaggedMessageType::Stories(msg) => {
				join_all(msg.iter().map(|story| story.handle(&client)))
					.await
					.into_iter()
					.find(|res| res.is_err())
					.unwrap_or(Ok(()))
			}
		}
	}
}

#[async_trait]
pub trait Message {
	async fn handle(&self, client: &Client) -> Result<(), Error>;
	async fn shared<T: ContentType + Send + Sync>(
		user: &User,
		content: &T,
		client: &Client,
	) -> Result<(), Error> {
		let settings = SETTINGS.wait();

		let username = &user.username;
		let notify = handle_content(content, client, user);
		let path = Path::new("data")
			.join(&username)
			.join(PostContent::get_type());
		let download = download_media(client, content.get_media(), &path);

		if settings.should_download(username) {
			if settings.should_notify(username) {
				return join(notify, download).await.0;
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
			client.fetch_content(&self.id),
		)
		.await?;

		Self::shared(&user, &content, client).await
	}
}

#[async_trait]
impl Message for ChatMessage {
	async fn handle(&self, client: &Client) -> Result<(), Error> {
		info!("Chat message received: {:?}", self);
		Self::shared(&self.from_user, &self.content, client).await
	}
}

#[async_trait]
impl Message for StoryMessage {
	async fn handle(&self, client: &Client) -> Result<(), Error> {
		info!("Story message received: {:?}", self);

		let user = client.fetch_user(&self.user_id.to_string()).await?;
		Self::shared(&user, &self.content, client).await
	}
}
