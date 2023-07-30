use crate::{MANAGER, SETTINGS, TEMPDIR, deserializers::{notification_message, from_string}, structs::ToToast, fetch_file};
use std::path::Path;
use anyhow::{bail, anyhow};
use filetime::{FileTime, set_file_mtime};
use of_client::{user::User, content, client::{OFClient, Authorized}, media::{Media, MediaType}, Url};
use serde::{Deserialize, Serialize};
use futures::future::{join, join_all};
use winrt_toast::{content::image::{ImageHintCrop, ImagePlacement}, Header, Image, Toast};

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
	pub async fn handle_message(self, client: &OFClient<Authorized>) -> anyhow::Result<()> {
		return match self {
			Self::Connected(msg) => {
				info!("Connected message received: {:?}", msg);
				Ok(())
			},
			Self::Onlines(_) => Ok(()),
			Self::Error(msg) => {
				error!("Error message received: {:?}", msg);
				bail!("websocket received error message with code {}", msg.error)
			},
			Self::NewMessage(msg) => {
				info!("Notification message received: {:?}", msg);
				handle(&msg.user, &msg.content, client).await
			},
			Self::Tagged(TaggedMessage::PostPublished(msg)) => {
				info!("Post message received: {:?}", msg);
				let content = client.get_post(msg.id).await?;
				
				let handle = handle(&content.author, &content, client);
				if SETTINGS.get().unwrap().lock().await.should_like::<content::Post>(&content.author.username) {
					join(handle, client.like_post(&content)).await.0
				} else {
					handle.await
				}
			},
			Self::Tagged(TaggedMessage::Api2ChatMessage(msg)) => {
				info!("Chat message received: {:?}", msg);

				let handle = handle(&msg.from_user, &msg.content, client);
				if SETTINGS.get().unwrap().lock().await.should_like::<content::Chat>(&msg.from_user.username) {
					join(handle, client.like_chat(&msg.content)).await.0
				} else {
					handle.await
				}
			},
			Self::Tagged(TaggedMessage::Stories(msg)) => {
				info!("Story message received: {:?}", msg);
				join_all(msg.iter().map(|story| async move {				
					let user = client.get_user(story.user_id).await?;

					let handle = handle(&user, &story.content, client);
					if SETTINGS.get().unwrap().lock().await.should_like::<content::Story>(&user.username) {
						join(handle, client.like_story(&story.content)).await.0
					} else {
						handle.await
					}
				}))
				.await
				.into_iter()
				.find(Result::is_err)
				.unwrap_or(Ok(()))
			},
			Self::Tagged(TaggedMessage::Stream(msg)) => {
				info!("Stream message received: {:?}", msg);
				handle(&msg.user, &msg.content, client).await
			}
		};
	}
}

async fn handle<T: content::Content + ToToast>(user: &User, content: &T, client: &OFClient<Authorized>) -> anyhow::Result<()> {
	let settings = SETTINGS.get().unwrap().lock().await;
	let header = T::content_type().to_string();

	let user_path = Path::new("data")
		.join(&user.username);

	let notify = async {
		if settings.should_notify::<T>(&user.username) {
			let mut toast: Toast = content.to_toast();
			toast
			.header(Header::new(&header, &header, ""))
			.text1(&user.name);

			if let Some(avatar) = &user.avatar {
				let filename = Url::parse(avatar)?
					.path_segments()
					.and_then(|segments| {
						let mut reverse_iter = segments.rev();
						let ext = reverse_iter.next().and_then(|file| file.split('.').last());
						let filename = reverse_iter.next();
			
						Option::zip(filename, ext).map(|(filename, ext)| [filename, ext].join("."))
					})
					.ok_or_else(|| anyhow!("Filename unknown"))?;
			
				let (_, avatar) = fetch_file(client, avatar, &user_path.join("Profile").join("Avatars"), Some(&filename)).await?;
				toast.image(1,
					Image::new_local(avatar.canonicalize()?)?
					.with_hint_crop(ImageHintCrop::Circle)
					.with_placement(ImagePlacement::AppLogoOverride),
				);
			}
			
			let thumb = content
				.media()
				.and_then(|media| {
					media.iter()
					.filter(|media| media.media_type() != &MediaType::Audio)
					.find_map(|media| media.thumbnail().filter(|s| !s.is_empty()))
				});

			if let Some(thumb) = thumb {
				let (_, thumb) = fetch_file(client, thumb, TEMPDIR.get().unwrap().path(), None).await?;
				toast.image(2, Image::new_local(thumb)?);
			}

			MANAGER.get().unwrap().show(&toast)?;
		}
		Ok(())
	};

	let download = async {
		if settings.should_notify::<T>(&user.username) && let Some(media) = content.media() {
			let _ = join_all(media.iter().filter_map(|media| {
				let path = user_path.join(&header).join(match media.media_type() {
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
		Ok(()) as anyhow::Result<()>
	};

	join(notify, download).await.0
}