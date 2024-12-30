use crate::{helpers::{fetch_file, get_avatar, get_thumbnail, show_notification}, settings::Settings};

use log::*;
use tokio::task::JoinHandle;
use std::{io, path::Path, sync::{Arc, RwLock}};
use anyhow::bail;
use ffmpeg_sidecar::command::FfmpegCommand;
use filetime::{set_file_mtime, FileTime};
use tempfile::TempDir;
use futures::{future::{join3, join_all, try_join}, FutureExt};
use nanohtml2text::html2text;
use of_daemon::structs::{Message, TaggedMessage};
use of_client::{content::{self, ContentType}, drm::MPDData, media::{Feed, Media, MediaType, DRM}, user::User, widevine::Cdm, OFClient};
use winrt_toast::{content::{image::{ImageHintCrop, ImagePlacement}, text::TextPlacement}, Header, Image, Text, Toast};

#[derive(Clone)]
pub struct Context {
	pub settings: Arc<RwLock<Settings>>,
	pub client: OFClient,
	device: Option<Cdm>,
	thumbnail_dir: Arc<TempDir>,
}

impl Context {
	pub fn new(client: OFClient, device: Option<Cdm>, settings: Arc<RwLock<Settings>>) -> Result<Self, io::Error> {
		let thumbnail_dir = TempDir::with_prefix("OF_thumbs")
		.inspect_err(|err| error!("Error creating temporary directory: {err}"))?;

		Ok(Self { client, device, settings, thumbnail_dir: Arc::new(thumbnail_dir) })
	}

	pub fn spawn_handle(&self, message: Message) -> anyhow::Result<Option<JoinHandle<()>>> {
		match message {
			Message::Error(msg) => {
				error!("Error message received: {:?}", msg);
				bail!("websocket received error message with code {}", msg.error)
			},
			Message::Notification(msg) => {
				info!("Notification message received: {:?}", msg);
				Ok(Some(tokio::spawn({
					let context = self.clone();
					async move { let _ = context.notify(&msg.content, &msg.user).await; }
				})))
			},
			Message::Tagged(TaggedMessage::Stream(msg)) => {
				info!("Stream message received: {:?}", msg);
				Ok(Some(tokio::spawn({
					let context = self.clone();
					async move { let _ = context.notify_with_thumbnail(&msg.content, &msg.user).await; }
				})))
			},
			Message::Tagged(TaggedMessage::PostPublished(msg)) => {
				info!("Post message received: {:?}", msg);

				Ok(Some(tokio::spawn({
					let context = self.clone();
					async move {
						if let Ok(content) = context.client.get_post(msg.id).await {
							join3(
								context.notify_with_thumbnail(&content, &content.author).map(|_| ()),
								context.download(&content, &content.author),
								context.like(&content, &content.author)
							).await;
						}
					}
				})))
			},
			Message::Tagged(TaggedMessage::Api2ChatMessage(msg)) => {
				info!("Chat message received: {:?}", msg);

				Ok(Some(tokio::spawn({
					let context = self.clone();
					async move {
						join3(
							context.notify_with_thumbnail(&msg.content, &msg.from_user).map(|_| ()),
							context.download(&msg.content, &msg.from_user),
							context.like(&msg.content, &msg.from_user)
						).await;
					}
				})))
			},
			Message::Tagged(TaggedMessage::Stories(msg)) => {
				info!("Story message received: {:?}", msg);

				Ok(Some(tokio::spawn({
					let context = self.clone();
					async move {
						join_all(msg.iter().map(|story| async {
							if let Ok(user) = context.client.get_user(story.user_id).await {
								join3(
									context.notify_with_thumbnail(&story.content, &user).map(|_| ()),
									context.download(&story.content, &user),
									context.like(&story.content, &user)
								).await;
							}
						})).await;
					}
				})))
			},
			_ => Ok(None)
		}
	}

	async fn notify<T: ToToast + content::Content>(&self, content: &T, user: &User) -> anyhow::Result<()> {
		if !(self.settings.read().unwrap().notify.enabled_for(&user.name, T::content_type())) { return Ok(()); }

		let mut toast = content.setup_notification(user);
		let avatar = get_avatar(user, &self.client).await?;
	
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

	async fn notify_with_thumbnail<T: ToToast + content::HasMedia>(&self, content: &T, user: &User) -> anyhow::Result<()> {
		if !(self.settings.read().unwrap().notify.enabled_for(&user.name, T::content_type())) { return Ok(()); }

		let mut toast = content.setup_notification(user);
		let (avatar, thumbnail) = try_join(get_avatar(user, &self.client), get_thumbnail(content, &self.client, self.thumbnail_dir.path())).await?;

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
	
	async fn download<T: content::HasMedia<Media = Feed>>(&self, content: &T, user: &User) {
		if !(self.settings.read().unwrap().download.enabled_for(&user.name, T::content_type())) { return; }

		let header = T::content_type().to_string();
		let content_path = Path::new("data").join(&user.username).join(&header);
	
		let _ = join_all(content.media().iter().map(|media| async {
			let path = content_path.join(match media.media_type() {
				MediaType::Photo => "Images",
				MediaType::Audio => "Audios",
				MediaType::Video | MediaType::Gif => "Videos",
			});
	
			if let Some(drm) = media.drm() && self.device.is_some() {
				let license_url = format!("https://onlyfans.com/api2/v2/users/media/{}/drm/{}/{}?type=widevine",
					media.id,
					match T::content_type() {
						ContentType::Chats => "message",
						_ => "post"
					},
					content.id()
				);
	
			self.download_media_drm(drm, &license_url, &path).await
			} else { self.download_media(media, &path).await }
		}))
		.await;
	}
	
	async fn download_media_drm(&self, media: &DRM, license_url: &str, path: &Path) -> anyhow::Result<()> {
		let MPDData { base_url: fname, pssh, last_modified } = self.client.get_mpd_data(media).await
			.inspect_err(|e| error!("{e}"))?;
	
		let key = self.client.get_decryption_key(self.device.as_ref().unwrap(), license_url, pssh).await
			.inspect_err(|e| error!("{e}"))?;
	
		let _ = tokio::task::spawn_blocking({
			let manifest = media.manifest.dash.clone();
			let mpd_header = self.client.mpd_header(&manifest);
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
	
	async fn download_media(&self, media: &Feed, path: &Path) -> anyhow::Result<()> {
		if let Some(url) = media.source() {
			let _ = fetch_file(&self.client, url, path, None)
				.await
				.inspect_err(|err| error!("Download failed: {err}"));
		}
	
		Ok(())
	}
	
	async fn like<T: content::CanLike>(&self, content: &T, user: &User) {
		if !(self.settings.read().unwrap().like.enabled_for(&user.name, T::content_type())) { return; }
		let _ = self.client.post(content.like_url(), None::<&[u8]>).await;
	}
	
}

trait ToToast {
	fn to_toast(&self) -> Toast;
	fn setup_notification(&self, user: &User) -> Toast
	where Self: content::Content,
	{
		let header = Self::content_type().to_string();
		let mut toast = self.to_toast();
		toast
		.header(Header::new(&header, &header, ""))
		.group(header)
		.tag(self.id().to_string())
		.timestamp(self.timestamp())
		.text1(&user.name);
	
		toast
	}
}

impl ToToast for content::Post {
	fn to_toast(&self) -> Toast {
		let mut toast = Toast::new();
		toast
		.text2(html2text(&self.text));

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
