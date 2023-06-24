use crate::{MANAGER, SETTINGS, TEMPDIR};
use crate::client::{OFClient, Authorized};
use crate::deserializers::notification_message;
use crate::structs::{content, user::User, media::{download_media, Media}};

use reqwest::Url;
use std::path::Path;
use anyhow::{anyhow, bail};
use serde::{Deserialize, Serialize};
use futures::future::{join, join_all};
use winrt_toast::{content::image::{ImageHintCrop, ImagePlacement}, Header, Image, Toast};

#[derive(Serialize, Debug)]
pub struct Connect<'a> {
	pub act: &'static str,
	pub token: &'a str,
}

#[derive(Serialize, Debug)]
pub struct Heartbeat {
	pub act: &'static str,
	pub ids: &'static [&'static u64],
}

impl Default for Heartbeat {
	fn default() -> Self {
		Heartbeat { act: "get_onlines", ids: &[] }
	}
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
	id: String,
	user_id: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Chat {
	from_user: User,
	#[serde(flatten)]
	content: content::Message,
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
				info!("Connect message received: {:?}", msg);

				let mut toast = Toast::new();
				toast.text1("OF Notifier").text2("Connection established");

				MANAGER.wait().show(&toast)?;
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
				let content = client.get_post(&msg.id).await?;
				
				let handle = handle(&content.author, &content, client);
				if SETTINGS.wait().should_like(&content.author.username) {
					join(handle, client.like_post(&content)).await.0
				} else {
					handle.await
				}
			},
			Self::Tagged(TaggedMessage::Api2ChatMessage(msg)) => {
				info!("Chat message received: {:?}", msg);

				let handle = handle(&msg.from_user, &msg.content, client);
				if SETTINGS.wait().should_like(&msg.from_user.username) {
					join(handle, client.like_message(&msg.content)).await.0
				} else {
					handle.await
				}
			},
			Self::Tagged(TaggedMessage::Stories(msg)) => {
				info!("Story message received: {:?}", msg);
				join_all(msg.iter().map(|story| async move {				
					let user = client.get_user(&story.user_id).await?;

					let handle = handle(&user, &story.content, client);
					if SETTINGS.wait().should_like(&user.username) {
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

async fn create_notification<T: content::Content>(content: &T, client: &OFClient<Authorized>, user: &User) -> anyhow::Result<()> {
	let avatar_url = user.avatar.parse::<Url>()?;
	let avatar_filename = avatar_url
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
			Some(&avatar_filename),
		)
		.await?;

	let header = <T as content::Content>::header();
	let mut toast: Toast = content.toast();
	toast
	.header(Header::new(header, header, ""))
	.text1(&user.name)
	.image(1,
		Image::new_local(avatar)?
		.with_hint_crop(ImageHintCrop::Circle)
		.with_placement(ImagePlacement::AppLogoOverride),
	);

	let thumb = content
		.media()
		.and_then(|media| {
			media.iter()
			.find_map(|media| media.thumbnail().filter(|s| !s.is_empty()))
		});

	if let Some(thumb) = thumb {
		let (_, thumb) = client.fetch_file(thumb, TEMPDIR.wait().path(), None).await?;
		toast.image(2, Image::new_local(thumb)?);
	}

	MANAGER.wait().show(&toast)?;
	Ok(())
}

async fn handle<T: content::Content + Send + Sync>(user: &User, content: &T, client: &OFClient<Authorized>) -> anyhow::Result<()> {
	let settings = SETTINGS.wait();

	let username = &user.username;
	let path = Path::new("data")
		.join(username)
		.join(<T as content::Content>::header());

	let notify = settings
		.should_notify(username)
		.then(|| {
			create_notification(content, client, user)
		});
	
	let download = settings
		.should_download(username)
		.then(|| {
			content
			.media()
			.map(|media| download_media(client, media, &path))
		}).flatten();
	
	return match (notify, download) {
		(Some(notify), Some(download)) => join(notify, download).await.0,
		(Some(notify), None) => notify.await,
		(None, Some(download)) => Ok(download.await),
		_ => Ok(())
	};
}

#[cfg(test)]
mod tests {
	use crate::{get_auth_params, settings::Settings, init, SETTINGS, client::OFClient};

	use std::sync::Once;
	use std::thread::sleep;
	use log::LevelFilter;
	use simplelog::{TermLogger, TerminalMode, Config, ColorChoice};
	use std::time::Duration;

	static INIT: Once = Once::new();

	fn test_init() {
		INIT.call_once(|| {
			init();

			SETTINGS
			.set(Settings::default())
			.unwrap();
	
			TermLogger::init(
				LevelFilter::Debug,
				Config::default(),
				TerminalMode::Mixed,
				ColorChoice::Auto,
			)
			.unwrap();
		});
	}

	#[tokio::test]
	async fn test_chat_message() {
		test_init();

		let incoming = r#"{
			"api2_chat_message": {
				"id": 0,
				"text": "This is a message<br />\n to test <a href = \"/onlyfans\">MARKDOWN parsing</a> 👌<br />\n in notifications 💯",
				"price": 3.99,
				"fromUser": {
					"avatar": "https://public.onlyfans.com/files/m/mk/mka/mkamcrf6rjmcwo0jj4zoavhmalzohe5a1640180203/avatar.jpg",
					"id": 15585607,
					"name": "OnlyFans",
					"username": "onlyfans"
				},
				"media": [
					{
						"id": 0,
						"canView": true,
						"src": "https://raw.githubusercontent.com/allenbenz/winrt-notification/main/resources/test/chick.jpeg",
						"preview": "https://raw.githubusercontent.com/allenbenz/winrt-notification/main/resources/test/flower.jpeg",
						"type": "photo"
					}
				]
			}
		}"#;

		let msg = serde_json::from_str::<super::Message>(incoming).unwrap();
		assert!(matches!(msg, super::Message::Tagged(super::TaggedMessage::Api2ChatMessage(_))));

		let params = get_auth_params().unwrap();
		let client = OFClient::new().authorize(params).await.unwrap();
		msg.handle_message(&client).await.unwrap();
		sleep(Duration::from_millis(1000));
	}

	#[tokio::test]
	async fn test_post_message() {
		test_init();

		// Onlyfan april fools post
		let incoming = r#"{
			"post_published": {
				"id": "129720708",
				"user_id" : "15585607",
				"show_posts_in_feed":true
			}
		}"#;

		let msg = serde_json::from_str::<super::Message>(incoming).unwrap();
		assert!(matches!(msg, super::Message::Tagged(super::TaggedMessage::PostPublished(_))));

		let params = get_auth_params().unwrap();
		let client = OFClient::new().authorize(params).await.unwrap();
		msg.handle_message(&client).await.unwrap();
		sleep(Duration::from_millis(1000));
	}

	#[tokio::test]
	async fn test_story_message() {
		test_init();

		let incoming = r#"{
			"stories": [
				{
					"id": 0,
					"userId": 15585607,
					"media":[
						{
							"id": 0,
							"canView": true,
							"files": {
								"source": {
									"url": "https://raw.githubusercontent.com/allenbenz/winrt-notification/main/resources/test/chick.jpeg"
								},
								"preview": {
									"url": "https://raw.githubusercontent.com/allenbenz/winrt-notification/main/resources/test/flower.jpeg"
								}
							},
							"type": "photo"
						}
					]
				}
			]
		}"#;

		let msg = serde_json::from_str::<super::Message>(incoming).unwrap();
		assert!(matches!(msg, super::Message::Tagged(super::TaggedMessage::Stories(_))));

		let params = get_auth_params().unwrap();
		let client = OFClient::new().authorize(params).await.unwrap();
		msg.handle_message(&client).await.unwrap();
		sleep(Duration::from_millis(1000));
	}

	
	#[tokio::test]
	async fn test_notification_message() {
		test_init();

		let incoming = r#"{
			"new_message":{
			   "id":"0",
			   "type":"message",
			   "text":"is currently running a promotion, <a href=\"https://onlyfans.com/onlyfans\">check it out</a>",
			   "subType":"promoreg_for_expired",
			   "user_id":"274000171",
			   "isRead":false,
			   "canGoToProfile":true,
			   "newPrice":null,
			   "user":{
					"avatar": "https://public.onlyfans.com/files/m/mk/mka/mkamcrf6rjmcwo0jj4zoavhmalzohe5a1640180203/avatar.jpg",
					"id": 15585607,
					"name": "OnlyFans",
					"username": "onlyfans"
				}
			},
			"hasSystemNotifications": false
		 }"#;

		let msg = serde_json::from_str::<super::Message>(incoming).unwrap();
		assert!(matches!(msg, super::Message::NewMessage(_)));

		let params = get_auth_params().unwrap();
		let client = OFClient::new().authorize(params).await.unwrap();
		msg.handle_message(&client).await.unwrap();
		sleep(Duration::from_millis(1000));
	}

	#[tokio::test]
	async fn test_stream_message() {
		test_init();

		let incoming = r#"{
			"stream": {
				"id": 2611175,
				"description": "stream description",
				"title": "stream title",
				"startedAt": "2022-11-05T14:02:24+00:00",
				"room": "dc2-room-7dYNFuya8oYBRs1",
				"thumbUrl": "https://stream1-dc2.onlyfans.com/img/dc2-room-7dYNFuya8oYBRs1/thumb.jpg",
				"user": {
					"avatar": "https://public.onlyfans.com/files/m/mk/mka/mkamcrf6rjmcwo0jj4zoavhmalzohe5a1640180203/avatar.jpg",
					"id": 15585607,
					"name": "OnlyFans",
					"username": "onlyfans"
				}
			}
		}"#;

		let msg = serde_json::from_str::<super::Message>(incoming).unwrap();
		assert!(matches!(msg, super::Message::Tagged(super::TaggedMessage::Stream(_))));

		let params = get_auth_params().unwrap();
		let client = OFClient::new().authorize(params).await.unwrap();
		msg.handle_message(&client).await.unwrap();
		sleep(Duration::from_millis(1000));
	}
}
