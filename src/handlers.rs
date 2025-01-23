use crate::{
	helpers::{fetch_file, filename_from_url, get_avatar, get_thumbnail, handle_download, show_notification},
	settings::{
		Settings,
		actions::{AllContent, DefaultActions, MediaContent},
		selection::{MediaContentSpecificSelection, MediaSelection, Selection},
	}
};

use log::*;
use reqwest::Url;
use tokio::{process as tProcess, task::JoinHandle};
use std::{io, iter::from_fn, path::Path, process, sync::{Arc, RwLock}};
use anyhow::{bail, anyhow};
use ffmpeg_sidecar::{command::FfmpegCommand, event::{FfmpegEvent, LogLevel}, log_parser::FfmpegLogParser};
use tempfile::TempDir;
use futures::{future::{join3, join_all, try_join, OptionFuture}, FutureExt};
use nanohtml2text::html2text;
use of_daemon::structs::{self, Message, TaggedMessage};
use of_client::{content::{self, CanLike, ContentType, HasMedia}, drm::MPDData, media::{Feed, Media, MediaType, Thumbnail, DRM}, user::User, widevine::Cdm, OFClient};
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

	async fn notify<T: content::Content + ToToast>(&self, content: &T, user: &User) -> anyhow::Result<()> {
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

	async fn notify_with_thumbnail<T: content::Content + content::HasMedia + ToToast>(&self, content: &T, user: &User) -> anyhow::Result<()> {
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
	
	async fn download<T: content::Content + content::HasMedia<Media = Feed>>(&self, content: &T, user: &User) {
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
		let MPDData { base_url: fname, pssh, last_modified } = self.client
			.get_mpd_data(media)
			.await
			.inspect_err(|err| error!("{err}"))?;

		let path = &path.join(fname);

		if  let Some(remote_modified) = last_modified &&
			let Ok(local_modified) = path.metadata().and_then(|metadata| metadata.modified()) &&
			local_modified >= remote_modified
		{
			return Ok(())
		}

		handle_download(path, last_modified, || async move {
			let key = self.client
				.get_decryption_key(self.device.as_ref().unwrap(), license_url, pssh)
				.await?;
			
			let manifest = &media.manifest.dash;

			let mut command: tProcess::Command = {
				let mut ffmpeg_command = FfmpegCommand::new();
				ffmpeg_command
				.hide_banner()
				.args(["-cenc_decryption_key", &base16::encode_lower(&key.key)])
				.args(["-headers", &self.client.mpd_header(manifest)])
				.overwrite()
				.input(manifest)
				.args(["-c", "copy"])
				.as_inner_mut()
				.arg(path);
	
				let std_command: process::Command = ffmpeg_command.into();
				std_command.into()
			};

			let output = command
				.spawn()?
				.wait_with_output()
				.await?;

			let mut log_parser = FfmpegLogParser::new(output.stderr.as_slice());
			let first_error = from_fn(|| match log_parser.parse_next_event() {
					Ok(entry) if !matches!(entry, FfmpegEvent::LogEOF) => Some(entry),
					_ => None,
				})
				.find(|entry| matches!(entry, FfmpegEvent::Log(LogLevel::Error, _)));

			if let Some(FfmpegEvent::Log(_, error)) = first_error {
				bail!(error)
			}

			Ok(())
		})
		.await
	}
	
	async fn download_media(&self, media: &Feed, path: &Path) -> anyhow::Result<()> {
		if let Some(url) = media.source() {
			let url = Url::parse(url)?;
			let filename = filename_from_url(&url)
				.ok_or_else(|| anyhow!("Filename unknown"))?;

			let path = path.join(filename);
			let _ = fetch_file(&self.client, url, &path).await;
		}
	
		Ok(())
	}
	
	async fn like<T: content::CanLike>(&self, content: &T) {
		let _ = self.client.post(content.like_url(), None::<&[u8]>).await;
	}
}

struct MediaContentActions {
	notify: bool,
	download: bool,
	like: bool,
}

trait Resolve<T> {
	type Resolved;
	fn resolve(&self, data: T) -> Self::Resolved;
}

impl <T, F, G> Resolve<(&T, F, G)> for DefaultActions
where
	T: HasMedia + CanLike,
	F: Fn(&AllContent) -> &Selection<MediaContentSpecificSelection>,
	G: Fn(&MediaContent) -> &Selection<MediaContentSpecificSelection>,
{
	type Resolved = MediaContentActions;
	fn resolve(&self, data: (&T, F, G)) -> Self::Resolved {
		let (content, content_selector, content_selector_media) = data;
		let has_media = !content.media().is_empty();
		let has_thumbnail = content.media().thumbnail().is_some();

		let resolver = |selection: &Selection<MediaContentSpecificSelection>| match selection {
			Selection::General(toggle) => **toggle,
			Selection::Specific(specific) => match specific.media {
				MediaSelection::Any => has_media,
				MediaSelection::Thumbnail => has_thumbnail,
				MediaSelection::None => !has_media,
			}
		};

		MediaContentActions {
			notify: match &self.notify {
				Selection::General(toggle) => **toggle,
				Selection::Specific(specific) => resolver(content_selector(specific))
			},
			download: match &self.download {
				Selection::General(toggle) => **toggle,
				Selection::Specific(specific) => resolver(content_selector_media(specific))
			},
			like: content.can_like() && match &self.like {
				Selection::General(toggle) => **toggle,
				Selection::Specific(specific) => resolver(content_selector_media(specific))
			}
		}
	}
}

pub trait Handler {
	fn handle(self, context: &Context) -> anyhow::Result<Option<JoinHandle<()>>>;
}

impl Handler for Message {
	fn handle(self, context: &Context) -> anyhow::Result<Option<JoinHandle<()>>> {
		match self {
			Message::Error(msg) => {
				error!("Error message received: {:?}", msg);
				bail!("websocket received error message with code {}", msg.error)
			},
			Message::Notification(msg) => {
				info!("Notification message received: {:?}", msg);
				msg.handle(context)
			},
			Message::Tagged(TaggedMessage::Stream(msg)) => {
				info!("Stream message received: {:?}", msg);
				msg.handle(context)
			},
			Message::Tagged(TaggedMessage::PostPublished(msg)) => {
				info!("Post message received: {:?}", msg);
				msg.handle(context)
			},
			Message::Tagged(TaggedMessage::Api2ChatMessage(msg)) => {
				info!("Chat message received: {:?}", msg);
				msg.handle(context)
			},
			Message::Tagged(TaggedMessage::Stories(msg)) => {
				info!("Story message received: {:?}", msg);
				msg.handle(context)
			},
			_ => Ok(None)
		}
	}
}

impl Resolve<&structs::Notification> for Settings {
	type Resolved = bool;
	fn resolve(&self, data: &structs::Notification) -> Self::Resolved {
		let actions = self.actions.get_actions_for(&data.user.username);
		match actions.notify {
			Selection::General(toggle) => *toggle,
			Selection::Specific(specific) => *specific.notifications
		}
	}
}

impl Handler for structs::Notification {
	fn handle(self, context: &Context) -> anyhow::Result<Option<JoinHandle<()>>> {
		Ok(
			context.settings.read().unwrap()
			.resolve(&self)
			.then(|| tokio::spawn({
				let context = context.clone();
				async move { let _ = context.notify(&self.content, &self.user).await; }
			})))
	}
}

impl Resolve<&structs::Stream> for Settings {
	type Resolved = bool;
	fn resolve(&self, data: &structs::Stream) -> Self::Resolved {
		let actions = self.actions.get_actions_for(&data.user.username);
		match actions.notify {
			Selection::General(toggle) => *toggle,
			Selection::Specific(specific) => *specific.streams
		}
	}
}

impl Handler for structs::Stream {
	fn handle(self, context: &Context) -> anyhow::Result<Option<JoinHandle<()>>> {
		Ok(
			context.settings.read().unwrap()
			.resolve(&self)
			.then(|| tokio::spawn({
				let context = context.clone();
				async move { let _ = context.notify_with_thumbnail(&self.content, &self.user).await; }
			})))
	}
}

impl Handler for structs::PostPublished {
	fn handle(self, context: &Context) -> anyhow::Result<Option<JoinHandle<()>>> {
		Ok(Some(tokio::spawn({
			let context = context.clone();
			async move {
				if let Ok(content) = context.client.get_post(self.id).await {
					let actions = context.settings.read().unwrap()
						.actions
						.get_actions_for(&content.author.username)
						.resolve((
							&content,
							|content: &AllContent| &content.posts,
							|content: &MediaContent| &content.posts,
						));

					join3(
						Into::<OptionFuture<_>>::into(actions.notify
						.then(|| context.notify_with_thumbnail(&content, &content.author).map(|_| ()))),
						Into::<OptionFuture<_>>::into(actions.download
						.then(|| context.download(&content, &content.author))),
						Into::<OptionFuture<_>>::into(actions.like
						.then(|| context.like(&content))),
					).await;
				}
			}
		})))
	}
}

impl Handler for structs::Chat {
	fn handle(self, context: &Context) -> anyhow::Result<Option<JoinHandle<()>>> {
		let actions = context.settings.read().unwrap()
		.actions
		.get_actions_for(&self.from_user.username)
		.resolve((
			&self.content,
			|content: &AllContent| &content.messages,
			|content: &MediaContent| &content.messages,
		));

		Ok(Some(tokio::spawn({
			let context = context.clone();
			async move {
				join3(
					Into::<OptionFuture<_>>::into(actions.notify
					.then(|| context.notify_with_thumbnail(&self.content, &self.from_user).map(|_| ()))),
					Into::<OptionFuture<_>>::into(actions.download
					.then(|| context.download(&self.content, &self.from_user))),
					Into::<OptionFuture<_>>::into(actions.like
					.then(|| context.like(&self.content))),
				).await;
			}
		})))
	}
}

impl Resolve<(&content::Story, &User)> for Settings {
	type Resolved = MediaContentActions;
	fn resolve(&self, data: (&content::Story, &User)) -> Self::Resolved {
		let (content, user) = data;
		let actions = self.actions.get_actions_for(&user.username);

		MediaContentActions {
			notify: match actions.notify {
				Selection::General(toggle) => *toggle,
				Selection::Specific(specific) => *specific.stories
			},
			download: match actions.download {
				Selection::General(toggle) => *toggle,
				Selection::Specific(specific) => *specific.stories
			},
			like: content.can_like() && match actions.like {
				Selection::General(toggle) => *toggle,
				Selection::Specific(specific) => *specific.stories
			}
		}
	}
}

impl Handler for Vec<structs::Story> {
	fn handle(self, context: &Context) -> anyhow::Result<Option<JoinHandle<()>>> {
		Ok(Some(tokio::spawn({
			let context = context.clone();
			async move {
				join_all(self.iter().map(|story| async {
					if let Ok(author) = context.client.get_user(story.user_id).await {
						let actions = context.settings.read().unwrap()
							.resolve((&story.content, &author));

						join3(
							Into::<OptionFuture<_>>::into(actions.notify
							.then(|| context.notify_with_thumbnail(&story.content, &author).map(|_| ()))),
							Into::<OptionFuture<_>>::into(actions.download
							.then(|| context.download(&story.content, &author))),
							Into::<OptionFuture<_>>::into(actions.like
							.then(|| context.like(&story.content))),
						).await;
					}
				})).await;
			}
		})))
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
