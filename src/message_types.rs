#![allow(dead_code)]

use crate::MANAGER;

use super::client::ClientExt;

use strip_markdown::*;
use reqwest::{Url, Client};
use chrono::{Utc, DateTime};
use std::{error, path::Path, process::Command};
use async_trait::async_trait;
use futures_util::future::try_join;
use futures::future::{try_join_all};
use filetime::{set_file_mtime, FileTime};
use serde::{Deserialize, Serialize, Deserializer};
use winrt_toast::{Toast, Header, Image, content::{image::{ImageHintCrop, ImagePlacement}, text::TextPlacement}, Text};

pub type Error = Box<dyn error::Error + Send + Sync>;

#[derive(Serialize, Debug)]
pub struct ConnectMessage<'a> {
	pub act: &'static str,
	pub token: &'a str
}

#[derive(Serialize, Debug)]
pub struct GetOnlinesMessage {
	pub act: &'static str,
	pub ids: &'static [&'static i32]
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct InitMessage<'a> {
	pub username: &'a str,
	pub ws_auth_token: &'a str
}

#[derive(Deserialize, Debug, Clone)]
pub struct ErrorMessage { 
	error: u8,
}

#[derive(Deserialize, Debug)]
pub struct ConnectedMessage { 
	connected: bool,
	v: String
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum MediaTypes {
	Photo,
	Video,
	Gif,
	Audio
}

#[derive(Debug)]
pub struct Media {
	id: u64,
	can_view: bool,
	created_at: DateTime<Utc>,
	source: Option<String>,
	preview: Option<String>,
	media_type: MediaTypes
}

fn de_markdown_string<'de, D>(deserializer: D) -> Result<String, D::Error> where D: Deserializer<'de> {
	let s = String::deserialize(deserializer)?;
	Ok(strip_markdown(&s))
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Content {
	id: u32,
	#[serde(deserialize_with = "de_markdown_string")]
	text: String,
	price: Option<f32>,
	#[serde(deserialize_with = "str_to_date")]
	posted_at: DateTime<Utc>,
	#[serde(deserialize_with = "media_from_content")]
	media: Vec<Media>
}

fn media_from_content<'de, D>(deserializer: D) -> Result<Vec<Media>, D::Error> where D: Deserializer<'de> {
	#[derive(Deserialize)]
	#[serde(rename_all = "camelCase")]
	struct Outer {
		id: u64,
		can_view: bool,
		#[serde(default = "Utc::now")]
		#[serde(deserialize_with = "str_to_date")]
		created_at: DateTime<Utc>,
		full: Option<String>,
		preview: Option<String>,
		#[serde(alias="type")]
		media_type: MediaTypes
	}

	<Vec<Outer>>::deserialize(deserializer)
	.map(|vec| {
		vec.into_iter().map(|outer| Media {
			id: outer.id,
			can_view: outer.can_view,
			created_at: outer.created_at,
			source: outer.full,
			preview: outer.preview,
			media_type: outer.media_type
		}).collect()
	})
}

fn str_to_date<'de, D>(deserializer: D) -> Result<DateTime<Utc>, D::Error> where D: Deserializer<'de> {
	let s = String::deserialize(deserializer)?;
	Ok(DateTime::parse_from_rfc3339(&s).map(|date| date.with_timezone(&Utc)).unwrap())
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct User {
	id: u32,
	name: String,
	username: String,
	avatar: String
}

#[derive(Deserialize, Debug)]
pub struct PostPublishedMessage { 
	id: String,
	user_id: String
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
	#[serde(deserialize_with = "de_markdown_string")]
	text: String,
	from_user: User,
	price: Option<f32>,
	#[serde(deserialize_with = "media_from_chat")]
	media: Vec<Media>
}

fn media_from_chat<'de, D>(deserializer: D) -> Result<Vec<Media>, D::Error> where D: Deserializer<'de> {
	#[derive(Deserialize)]
	#[serde(rename_all = "camelCase")]
	struct Outer {
		id: u64,
		can_view: bool,
		#[serde(default = "Utc::now")]
		#[serde(deserialize_with = "str_to_date")]
		created_at: DateTime<Utc>,
		src: Option<String>,
		preview: Option<String>,
		#[serde(alias="type")]
		media_type: MediaTypes
	}

	<Vec<Outer>>::deserialize(deserializer)
	.map(|vec| {
		vec.into_iter().map(|outer| Media {
			id: outer.id,
			can_view: outer.can_view,
			created_at: outer.created_at,
			source: outer.src,
			preview: outer.preview,
			media_type: outer.media_type
		}).collect()
	})
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct StoryMessage {
	id: u32,
	user_id: u32,
	#[serde(deserialize_with = "media_from_story")]
	media: Vec<Media>
}

fn media_from_story<'de, D>(deserializer: D) -> Result<Vec<Media>, D::Error> where D: Deserializer<'de> {
	#[derive(Deserialize)]
	#[serde(rename_all = "camelCase")]
	struct Outer {
		id: u64,
		can_view: bool,
		#[serde(default = "Utc::now")]
		#[serde(deserialize_with = "str_to_date")]
		created_at: DateTime<Utc>,
		files: Option<Inner>,
		#[serde(alias="type")]
		media_type: MediaTypes
	}

	#[derive(Deserialize, Clone)]
	struct Inner {
		source: Option<UrlInner>,
		preview: Option<UrlInner>
	}

	#[derive(Deserialize, Clone)]
	struct UrlInner {
		url: Option<String>
	}

	<Vec<Outer>>::deserialize(deserializer)
	.map(|vec| {
		vec.into_iter().map(|outer| Media {
			id: outer.id,
			can_view: outer.can_view,
			created_at: outer.created_at,
			source: outer.files.clone().and_then(|inner| inner.source).and_then(|source| source.url),
			preview: outer.files.and_then(|inner| inner.preview).and_then(|preview| preview.url),
			media_type: outer.media_type
		}).collect()
	})
}

#[async_trait]
trait _Toast {
	async fn toast(&self, client: &Client, user_path: &Path) -> Result<Toast, Error>;
}

#[async_trait]
impl _Toast for Content {
	async fn toast(&self, client: &Client, user_path: &Path) -> Result<Toast, Error> {
		let content_type = "Posts";

		let mut toast = Toast::new();
		toast
		.header(Header::new(content_type, content_type, content_type))
		.launch(user_path.join(content_type).to_str().unwrap())
		.text2(self.text.clone());

		info!("{}", self.text);

		if let Some(price) = self.price {
			toast.text3(Text::new(format!("${:.2}", price))
				.with_placement(TextPlacement::Attribution));
		}

		if let Some(thumb) = self.media.iter().filter_map(|media| media.preview.as_deref()).next() {
			let thumb = client.fetch_file(thumb, &user_path.join("thumbs"), None).await?;
			toast.image(2, Image::new_local(thumb)?);
		}

		Ok(toast)
	}
}

#[async_trait]
impl _Toast for ChatMessage {
	async fn toast(&self, client: &Client, user_path: &Path) -> Result<Toast, Error> {
		let content_type = "Messages";

		let mut toast = Toast::new();
		toast
		.header(Header::new(content_type, content_type, content_type))
		.launch(user_path.join(content_type).to_str().unwrap())
		.text2(self.text.clone());

		info!("{}", self.text);

		if let Some(price) = self.price {
			toast.text3(Text::new(format!("${:.2}", price))
				.with_placement(TextPlacement::Attribution));
		}

		if let Some(thumb) = self.media.iter().filter_map(|media| media.preview.as_deref()).next() {
			let thumb = client.fetch_file(thumb, &user_path.join("thumbs"), None).await?;
			toast.image(2, Image::new_local(thumb)?);
		}

		Ok(toast)
	}
}

#[async_trait]
impl _Toast for StoryMessage {
	async fn toast(&self, client: &Client, user_path: &Path) -> Result<Toast, Error> {
		let content_type = "Stories";

		let mut toast = Toast::new();
		toast
		.header(Header::new(content_type, content_type, content_type))
		.launch(user_path.join(content_type).to_str().unwrap());

		if let Some(thumb) = self.media.iter().filter_map(|media| media.preview.as_deref()).next() {
			let thumb = client.fetch_file(thumb, &user_path.join("thumbs"), None).await?;
			toast.image(2, Image::new_local(thumb)?);
		}

		Ok(toast)
	}
}


#[derive(Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum TaggedMessageType {
	PostPublished(PostPublishedMessage),
	Api2ChatMessage(ChatMessage),
	Stories(Vec<StoryMessage>)
}

#[derive(Deserialize, Debug)]
#[serde(untagged)]
pub enum MessageType {
	Tagged(TaggedMessageType),
	Connected(ConnectedMessage),
	Error(ErrorMessage),
}

impl MessageType {
	async fn download_media(client: &Client, media: &Vec<Media>, path: &Path) -> Result<(), Error> {
		try_join_all(media.iter().filter_map(|media| {
			media.source.as_ref().map(|url| async move {
				client.fetch_file(url, 
					&path.join(match media.media_type {
						MediaTypes::Photo => "Images",
						MediaTypes::Audio => "Audios",
						MediaTypes::Video | MediaTypes::Gif => "Videos"
					}),
					None).await
					.and_then(|path| {
						set_file_mtime(path, FileTime::from_unix_time(media.created_at.timestamp(), 0))
						.map_err(|err| err.into())
					})
			})
		})).await
		.map(|_| ())
	}

	async fn handle_content(client: &Client, user: &User, content: &impl _Toast) -> Result<(), Error> {
		let parsed = user.avatar.parse::<Url>()?;
		let filename = parsed.path_segments()
			.and_then(|segments|  {
				let mut reverse_iter = segments.rev();
				let ext = reverse_iter.next().and_then(|file| file.split('.').last());
				let filename = reverse_iter.next();

				Option::zip(filename, ext).map(|(filename, ext)| [filename, ext].join("."))
			}).ok_or("Filename unknown")?;

		let user_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("data").join(&user.username);
		let avatar = client.fetch_file(&user.avatar, &user_path.join("Profile").join("Avatars"), Some(&filename)).await?;

		info!("Creating notification");
		let mut toast = content.toast(client, &user_path).await?;
		toast
		.text1(&user.name)
		.image(1, 
			Image::new_local(avatar)?
			.with_hint_crop(ImageHintCrop::Circle)
			.with_placement(ImagePlacement::AppLogoOverride)
		);

		MANAGER.show_with_callbacks(&toast,
			Some(Box::new(move |rs| { 
				if let Ok(s) = rs {
					Command::new("explorer")
					.arg(s)
					.spawn()
					.unwrap();
				}
			})),
			None,
			Some(Box::new(move |e| { error!("Could't show notification: {:?}", e); })))
		.map_err(|err| err.into())
	}

	pub async fn handle_message(&self) -> Result<(), Error> {
		return match self {
			Self::Connected(_) => {
				info!("Connect message received");

				let mut toast = Toast::new();
				toast
				.text1("OF Notifier")
				.text2("Connection established");

				MANAGER.show(&toast)?;
				Ok(())
			},
			Self::Error(msg) =>  {
				error!("Error message received: {:?}", msg);
				Err(format!("websocket received error message with code {}", msg.error).into())
			},
			Self::Tagged(tagged) => {
				let client = Client::with_auth().await?;

				match tagged {
					TaggedMessageType::PostPublished(msg) => {
						info!("Post message received: {:?}", msg);
		
						let (user, content) = try_join(client.fetch_user(&msg.user_id), client.fetch_content(&msg.id)).await?;
						try_join(
							Self::handle_content(&client, &user, &content),
							Self::download_media(&client, &content.media, &Path::new("data").join(&user.username).join("Posts"))
						).await
						.map(|_| ())
					},
					TaggedMessageType::Api2ChatMessage(msg) => {
						info!("Chat message received: {:?}", msg);
		
						try_join(
							Self::handle_content(&client, &msg.from_user, msg),
							Self::download_media(&client, &msg.media, &Path::new("data").join(&msg.from_user.username).join("Messages"))
						).await
						.map(|_| ())
					},
					TaggedMessageType::Stories(msg) => {
						info!("Story message received: {:?}", msg);
		
						try_join_all(msg.iter().map(|msg| async {
							let user = client.fetch_user(&msg.user_id.to_string()).await?;
							try_join(
								Self::handle_content(&client, &user, msg),
								Self::download_media(&client, &msg.media, &Path::new("data").join(&user.username).join("Stories"))
							).await
						})).await
						.map(|_| ())
					}

				}
			}
		};
	}
}