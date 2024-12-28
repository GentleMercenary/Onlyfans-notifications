use crate::{helpers::{fetch_file, get_avatar, get_thumbnail, show_notification}, settings::Settings};

use std::path::Path;
use anyhow::bail;
use ffmpeg_sidecar::command::FfmpegCommand;
use filetime::{set_file_mtime, FileTime};
use tokio::sync::RwLock;
use futures::{future::{join_all, try_join}, FutureExt};
use nanohtml2text::html2text;
use of_socket::structs::{Message, TaggedMessage};
use of_client::{content::{self, ContentType}, drm::MPDData, media::{Feed, Media, MediaType, DRM}, user::User, widevine::Cdm, OFClient};
use winrt_toast::{content::{image::{ImageHintCrop, ImagePlacement}, text::TextPlacement}, Header, Image, Text, Toast};

pub async fn handle_message(message: Message, client: &OFClient, settings: &RwLock<Settings>, device: Option<&Cdm>) -> anyhow::Result<()> {
	match message {
		Message::Connected(_) | Message::Onlines(_) => Ok(()),
		Message::Error(msg) => {
			error!("Error message received: {:?}", msg);
			bail!("websocket received error message with code {}", msg.error)
		},
		_ => {
			match message {
				Message::Notification(msg) => {
					info!("Notification message received: {:?}", msg);

					let settings = settings.read().await;
					if settings.notify.enabled_for(&msg.user.username, ContentType::Notifications) {
						let _ = notify(&msg.content, &msg.user, client).await;
					}
				},
				Message::Tagged(TaggedMessage::Stream(msg)) => {
					info!("Stream message received: {:?}", msg);
					
					let settings = settings.read().await;
					if settings.notify.enabled_for(&msg.user.username, ContentType::Streams) {
						let _ = notify(&msg.content, &msg.user, client).await;
					}
				},
				Message::Tagged(TaggedMessage::PostPublished(msg)) => {
					info!("Post message received: {:?}", msg);
					let content = client.get_post(msg.id).await?;

					let settings = settings.read().await;
					let futs = [
						settings.notify.enabled_for(&content.author.username, ContentType::Posts)
							.then(|| notify_with_thumbnail(&content, &content.author, client).map(|_| ()).boxed()),
						settings.download.enabled_for(&content.author.username, ContentType::Posts)
							.then(|| download(&content, &content.author, client, device).boxed()),
						settings.like.enabled_for(&content.author.username, ContentType::Posts)
							.then(|| like(&content, client).boxed())
					]
					.into_iter()
					.flatten();
					
					drop(settings);
					let _ = join_all(futs).await;
				},
				Message::Tagged(TaggedMessage::Api2ChatMessage(msg)) => {
					info!("Chat message received: {:?}", msg);

					let settings = settings.read().await;
					let futs = [
						settings.notify.enabled_for(&msg.from_user.username, ContentType::Chats)
							.then(|| notify_with_thumbnail(&msg.content, &msg.from_user, client).map(|_| ()).boxed()),
						settings.download.enabled_for(&msg.from_user.username, ContentType::Chats)
							.then(|| download(&msg.content, &msg.from_user, client, device).boxed()),
						settings.like.enabled_for(&msg.from_user.username, ContentType::Chats)
							.then(|| like(&msg.content, client).boxed())
					]
					.into_iter()
					.flatten();

					drop(settings);
					let _ = join_all(futs).await;
				},
				Message::Tagged(TaggedMessage::Stories(msg)) => {
					info!("Story message received: {:?}", msg);
					
					let _ = join_all(msg.iter().map(|story| async move {
						if let Ok(user) = client.get_user(story.user_id).await {
							let settings = settings.read().await;
							let futs = [
								settings.notify.enabled_for(&user.username, ContentType::Stories)
									.then(|| notify_with_thumbnail(&story.content, &user, client).map(|_| ()).boxed()),
								settings.download.enabled_for(&user.username, ContentType::Stories)
									.then(|| download(&story.content, &user, client, device).boxed()),
								settings.like.enabled_for(&user.username, ContentType::Stories)
									.then(|| like(&story.content, client).boxed())
							]
							.into_iter()
							.flatten();
	
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

fn setup_notification<T: ToToast + content::Content>(content: &T, user: &User) -> Toast {
	let header = T::content_type().to_string();
	let mut toast = content.to_toast();

	toast
	.header(Header::new(&header, &header, ""))
	.timestamp(content.timestamp())
	.text1(&user.name);

	toast
}

async fn notify<T: ToToast + content::Content>(content: &T, user: &User, client: &OFClient) -> anyhow::Result<()> {
	let avatar = get_avatar(user, client).await?;

	let mut toast = setup_notification(content, user);

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
	let (avatar, thumbnail) = try_join(get_avatar(user, client), get_thumbnail(content, client)).await?;
	
	let mut toast = setup_notification(content, user);

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

async fn download<T: content::HasMedia<Media = Feed>>(content: &T, user: &User, client: &OFClient, cdm: Option<&Cdm>) {
	let header = T::content_type().to_string();
	let content_path = Path::new("data").join(&user.username).join(&header);

	let _ = join_all(content.media().iter().map(|media| async {
		let path = content_path.join(match media.media_type() {
			MediaType::Photo => "Images",
			MediaType::Audio => "Audios",
			MediaType::Video | MediaType::Gif => "Videos",
		});

		if let Some(drm) = media.drm() && let Some(cdm) = cdm {
			let license_url = format!("https://onlyfans.com/api2/v2/users/media/{}/drm/{}/{}?type=widevine",
				media.id,
				match T::content_type() {
					ContentType::Chats => "message",
					_ => "post"
				},
				content.id()
			);

		let _ = download_media_drm(drm, &license_url, &path, client, cdm).await;
		} else { let _ = download_media(media, &path, client).await; }
	}))
	.await;
}

async fn download_media_drm(media: &DRM, license_url: &str, path: &Path, client: &OFClient, cdm: &Cdm) -> anyhow::Result<()> {
	let MPDData { base_url: fname, pssh, last_modified } = client.get_mpd_data(media).await
		.inspect_err(|e| error!("{e}"))?;

	let key = client.get_decryption_key(cdm, license_url, pssh).await
		.inspect_err(|e| error!("{e}"))?;

	let _ = tokio::task::spawn_blocking({
		let manifest = media.manifest.dash.clone();
		let mpd_header = client.mpd_header(&manifest);
		let out_path = path.join(fname);
		move || {
			let result = FfmpegCommand::new()
				.hide_banner()
				.create_no_window()
				.args(["-cenc_decryption_key", &base16::encode_lower(&key.key)])
				.args(["-headers", &mpd_header])
				.overwrite()
				.input(&manifest)
				.args(["-c", "copy"])
				.output(out_path.to_string_lossy())
				.spawn()
				.and_then(|mut command| command.wait())
				.inspect_err(|e| warn!("FFmpeg command failed: ${e}"));
			
			if let Ok(_) = result && let Some(time) = last_modified {
				let _ = set_file_mtime(&out_path, FileTime::from_system_time(time))
				.inspect_err(|e| warn!("Error setting file modified date: ${e}"));
			}
		}
	}).await;

	Ok(())
}

async fn download_media(media: &Feed, path: &Path, client: &OFClient) -> anyhow::Result<()> {
	if let Some(url) = media.source() {
		let _ = fetch_file(client, url, path, None)
			.await
			.inspect_err(|err| error!("Download failed: {err}"));
	}

	Ok(())
}

async fn like<T: content::CanLike>(content: &T, client: &OFClient) {
	let _ = client.post(content.like_url(), None::<&[u8]>).await;
}
