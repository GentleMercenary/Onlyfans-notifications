use crate::{helpers::{fetch_file, get_avatar, get_thumbnail, show_notification}, settings::Settings, structs::{Message, TaggedMessage}};

use std::path::Path;
use anyhow::bail;
use tokio::sync::RwLock;
use futures::{future::{join_all, try_join, BoxFuture}, FutureExt};
use nanohtml2text::html2text;
use of_client::{client::OFClient, content::{self, ContentType}, media::{Media, MediaType}, user::User};
use winrt_toast::{content::{image::{ImageHintCrop, ImagePlacement}, text::TextPlacement}, Header, Image, Text, Toast};

pub async fn handle_message(message: Message, client: &OFClient, settings: &RwLock<Settings>) -> anyhow::Result<()> {
	match message {
		Message::Connected(_) | Message::Onlines(_) => Ok(()),
		Message::Error(msg) => {
			error!("Error message received: {:?}", msg);
			bail!("websocket received error message with code {}", msg.error)
		},
		_ => {
			match message {
				Message::NewMessage(msg) => {
					info!("Notification message received: {:?}", msg);

					let settings = settings.read().await;
					if settings.notify.enabled_for(&msg.user.username, ContentType::Notifications) {
						let _ = notify(&msg.content, &msg.user, client).await;
					}
				},
				Message::Tagged(TaggedMessage::Stream(msg)) => {
					info!("Stream message received: {:?}", msg);
					
					let settings = settings.read().await;
					let futs: Vec<BoxFuture<'_, ()>> = vec![
						settings.notify.enabled_for(&msg.user.name, ContentType::Streams)
							.then(|| notify_with_thumbnail(&msg.content, &msg.user, client).map(|_| ()).boxed()),
						settings.download.enabled_for(&msg.user.username, ContentType::Streams)
							.then(|| download(&msg.content, &msg.user, client).boxed())
					]
					.into_iter()
					.flatten()
					.collect();

					drop(settings);
					let _ = join_all(futs).await;
				},
				Message::Tagged(TaggedMessage::PostPublished(msg)) => {
					info!("Post message received: {:?}", msg);
					let content = client.get_post(msg.id).await?;

					let settings = settings.read().await;
					let futs: Vec<BoxFuture<'_, ()>> = vec![
						settings.notify.enabled_for(&content.author.username, ContentType::Posts)
							.then(|| notify_with_thumbnail(&content, &content.author, client).map(|_| ()).boxed()),
						settings.download.enabled_for(&content.author.username, ContentType::Posts)
							.then(|| download(&content, &content.author, client).boxed()),
						settings.like.enabled_for(&content.author.username, ContentType::Posts)
							.then(|| like(&content, client).boxed())
					]
					.into_iter()
					.flatten()
					.collect();
					
					drop(settings);
					let _ = join_all(futs).await;
				},
				Message::Tagged(TaggedMessage::Api2ChatMessage(msg)) => {
					info!("Chat message received: {:?}", msg);

					let settings = settings.read().await;
					let futs: Vec<BoxFuture<'_, ()>> = vec![
						settings.notify.enabled_for(&msg.from_user.username, ContentType::Chats)
							.then(|| notify_with_thumbnail(&msg.content, &msg.from_user, client).map(|_| ()).boxed()),
						settings.download.enabled_for(&msg.from_user.username, ContentType::Chats)
							.then(|| download(&msg.content, &msg.from_user, client).boxed()),
						settings.like.enabled_for(&msg.from_user.username, ContentType::Chats)
							.then(|| like(&msg.content, client).boxed())
					]
					.into_iter()
					.flatten()
					.collect();

					drop(settings);
					let _ = join_all(futs).await;
				},
				Message::Tagged(TaggedMessage::Stories(msg)) => {
					info!("Story message received: {:?}", msg);
					
					let _ = join_all(msg.iter().map(|story| async move {
						if let Ok(user) = client.get_user(story.user_id).await {
							let settings = settings.read().await;
							let futs: Vec<BoxFuture<'_, ()>> = vec![
								settings.notify.enabled_for(&user.username, ContentType::Stories)
									.then(|| notify_with_thumbnail(&story.content, &user, client).map(|_| ()).boxed()),
								settings.download.enabled_for(&user.username, ContentType::Stories)
									.then(|| download(&story.content, &user, client).boxed()),
								settings.like.enabled_for(&user.username, ContentType::Stories)
									.then(|| like(&story.content, client).boxed())
							]
							.into_iter()
							.flatten()
							.collect();
	
							drop(settings);
							let _ = join_all(futs).await;
						}
					}))
					.await;
				},
				_ => () // unhandled
			}
			Ok(())
		}
	}
}

trait ToToast {
	fn to_toast(&self) -> Toast;
}

impl ToToast for content::Post {
	fn to_toast(&self) -> Toast {
		let mut toast = Toast::new();
		toast.text2(html2text(&self.text));

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
		toast.text2(html2text(&self.text));

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
		toast.text2(html2text(&self.text));
		toast
	}
}

impl ToToast for content::Stream {
	fn to_toast(&self) -> Toast {
		let mut toast = Toast::new();
		toast.text2(html2text(&self.description));
		toast
	}
}

async fn notify<T: ToToast + content::Content>(content: &T, user: &User, client: &OFClient) -> anyhow::Result<()> {
	let header = T::content_type().to_string();
	let mut toast = content.to_toast();

	let avatar = get_avatar(user, client).await?;

	toast
	.header(Header::new(&header, &header, ""))
	.text1(&user.name);

	if let Some(avatar) = avatar {
		toast.image(1, 
			Image::new_local(avatar.canonicalize()?)?
			.with_hint_crop(ImageHintCrop::Circle)
			.with_placement(ImagePlacement::AppLogoOverride)
		);
	}

	show_notification(&toast)?;
	Ok(())
}

async fn notify_with_thumbnail<T: ToToast + content::HasMedia>(content: &T, user: &User, client: &OFClient) -> anyhow::Result<()> {
	let header = T::content_type().to_string();
	let mut toast = content.to_toast();

	let (avatar, thumbnail) = try_join(get_avatar(user, client), get_thumbnail(content, client)).await?;

	toast
	.header(Header::new(&header, &header, ""))
	.text1(html2text(&user.name));

	if let Some(avatar) = avatar {
		toast.image(1, 
			Image::new_local(avatar.canonicalize()?)?
			.with_hint_crop(ImageHintCrop::Circle)
			.with_placement(ImagePlacement::AppLogoOverride)
		);
	}

	if let Some(thumbnail) = thumbnail {
		toast.image(2, Image::new_local(thumbnail)?);
	}

	show_notification(&toast)?;
	Ok(())
}

async fn download<T: content::HasMedia>(content: &T, user: &User, client: &OFClient) {
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
		})
	}))
	.await;
}

async fn like<T: content::CanLike>(content: &T, client: &OFClient) {
	let _ = client.post(content.like_url(), None::<&()>).await;
}
