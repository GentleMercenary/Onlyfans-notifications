use log::*;
use reqwest::header::{HeaderValue, IF_MODIFIED_SINCE};
use tokio::{fs as tfs, io::copy_buf};
use tokio_util::io::StreamReader;
use std::{fs, future::Future, io::{Error, ErrorKind}, path::{Path, PathBuf}, sync::{Mutex, OnceLock}, time::SystemTime};
use anyhow::{anyhow, Context};
use filetime::{set_file_mtime, FileTime};
use futures::TryStreamExt;
use httpdate::{fmt_http_date, parse_http_date};
use of_client::{content, media::Thumbnail, reqwest::{header, IntoUrl, StatusCode, Url}, user::User, OFClient};
use winrt_toast::{register, Toast, ToastManager};

pub fn filename_from_url(url: &Url) -> Option<&str> {
	url
	.path_segments()
	.and_then(Iterator::last)
	.and_then(|name| (!name.is_empty()).then_some(name))
}

pub async fn get_avatar(user: &User, client: &OFClient) -> anyhow::Result<Option<PathBuf>> {
	match &user.avatar {
		Some(avatar) => {
			let avatar_url = Url::parse(avatar)?;
			let (filename, ext) = avatar_url
				.path_segments()
				.and_then(|segments| {
					let mut reverse_iter = segments.rev();
					let ext = reverse_iter.next().and_then(|file| file.split('.').last());
					let filename = reverse_iter.next();
		
					Option::zip(filename, ext)
				})
				.ok_or_else(|| anyhow!("Filename unknown"))?;
	
			let path = Path::new("data")
				.join(&user.username)
				.join("Profile")
				.join("Avatars")
				.join(filename)
				.with_extension(ext);

			fetch_file(client, avatar, &path).await?;
			Ok(Some(path))
			
		},
		None => Ok(None)
	}
}

pub async fn get_thumbnail<T: content::HasMedia>(content: &T, client: &OFClient, temp_dir: &Path) -> anyhow::Result<Option<PathBuf>> {
	let media = content.media();
	match media.thumbnail() {
		Some(thumb) => {
			let thumbnail_url = Url::parse(thumb)?;
			let filename = filename_from_url(&thumbnail_url)
				.ok_or_else(|| anyhow!("Filename unknown"))?;

			let path = temp_dir.join(filename);
			fetch_file(client, thumb, &path).await?;
			Ok(Some(path))
		},
		None => Ok(None)
	}
}

pub async fn handle_download<'a, F, Fut>(path: &'a Path, modified: Option<SystemTime>, fetch_fn: F) -> anyhow::Result<()>
where
	F: FnOnce() -> Fut,
	Fut: Future<Output = anyhow::Result<()>> + 'a,
{
	if let Some(parent) = path.parent() { fs::create_dir_all(parent)?; }

	fetch_fn().await
	.inspect_err(|err| error!("Downloading {:?} failed: {err}", path.file_name().unwrap()))?;

	if let Some(date) = modified {
		set_file_mtime(path, FileTime::from_system_time(date))
		.context("Setting file modified date")?;
	}

	Ok(())
}

pub async fn fetch_file<U: IntoUrl>(client: &OFClient, link: U, path: &Path) -> anyhow::Result<()> {
	let url = link.into_url()?;

	let response = match path.metadata().and_then(|metadata| metadata.modified()) {
		Ok(date) => {
			let response = client.get(url)
			.header(IF_MODIFIED_SINCE, HeaderValue::from_str(&fmt_http_date(date)).unwrap())
			.send()
			.await?;

			if response.status() == StatusCode::NOT_MODIFIED { return Ok(()) }
			response
		},
		Err(_) => client.get(url).send().await?
	};

	let modified = response
		.headers()
		.get(header::LAST_MODIFIED)
		.and_then(|header| header.to_str().ok())
		.and_then(|s| parse_http_date(s).ok());

	handle_download(path, modified, || async move {
		let temp_path = path.with_extension("temp");
		let mut file = tfs::File::from_std(fs::File::create(&temp_path)?);
		let mut reader = StreamReader::new(
			response
			.bytes_stream()
			.map_err(|e| Error::new(ErrorKind::Other, e))
		);
	
		copy_buf(&mut reader, &mut file).await?;
	
		fs::rename(&temp_path, path).map_err(Into::into)
	}).await
	.inspect_err(|err| error!("Download failed: {err}"))
}

pub fn show_notification(toast: &Toast) -> winrt_toast::Result<()> {
	static MANAGER: OnceLock<Mutex<ToastManager>> = OnceLock::new();
	let manager_mutex = MANAGER.get_or_init(|| {
		let aum_id = "OFNotifier";
		let icon_path = Path::new("icons").join("icon.ico").canonicalize()
			.inspect_err(|err| error!("{err}"))
			.unwrap();
	
		register(aum_id, "OF notifier", Some(icon_path.as_path()))
		.inspect_err(|err| error!("{err}"))
		.unwrap();
		
		Mutex::new(ToastManager::new(aum_id))
	});

	let manager = manager_mutex.lock().unwrap();
	manager.show(toast)
}