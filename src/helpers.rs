#![allow(dead_code)]

use std::{fs::{self, File}, io::{BufWriter, Write}, path::{Path, PathBuf}, sync::{Mutex, OnceLock}};

use anyhow::{anyhow, Context, Ok};
use filetime::{set_file_mtime, FileTime};
use futures_util::StreamExt;
use of_client::{client::OFClient, content, httpdate::parse_http_date, media::{Media, MediaType}, reqwest::{header, IntoUrl, StatusCode, Url}, user::User};
use tempdir::TempDir;
use winrt_toast::{register, Toast, ToastManager};

use crate::get_auth_params;

pub fn init_client() -> anyhow::Result<OFClient> {
	info!("Reading authentication parameters");
	let auth_params = get_auth_params()?;
	let client = OFClient::new(auth_params)?;
	Ok(client)
}

pub async fn get_avatar(user: &User, client: &OFClient) -> anyhow::Result<Option<PathBuf>> {
	match &user.avatar {
		Some(avatar) => {
			let filename = Url::parse(avatar)?
				.path_segments()
				.and_then(|segments| {
					let mut reverse_iter = segments.rev();
					let ext = reverse_iter.next().and_then(|file| file.split('.').last());
					let filename = reverse_iter.next();
		
					Option::zip(filename, ext).map(|(filename, ext)| [filename, ext].join("."))
				})
				.ok_or_else(|| anyhow!("Filename unknown"))?;
	
			let user_path = Path::new("data").join(&user.username);
			let (_, avatar) = fetch_file(client, avatar, &user_path.join("Profile").join("Avatars"), Some(&filename)).await?;
			Ok(Some(avatar))
		},
		None => Ok(None)
	}
}

pub async fn get_thumbnail<T: content::HasMedia>(content: &T, client: &OFClient) -> anyhow::Result<Option<PathBuf>> {
	let thumb = content
	.media()
	.iter()
	.filter(|media| media.media_type() != &MediaType::Audio)
	.find_map(|media| media.thumbnail().filter(|s| !s.is_empty()));

	match thumb {
		Some(thumb) => {
			static TEMPDIR: OnceLock<TempDir> = OnceLock::new();
			let temp_dir = TEMPDIR.get_or_init(|| TempDir::new("OF_thumbs").expect("Creating temporary directory"));
			let (_, path) = fetch_file(client, thumb, temp_dir.path(), None).await?;
			Ok(Some(path))
		},
		None => Ok(None)
	}

}

pub async fn fetch_file<U: IntoUrl>(client: &OFClient, link: U, path: &Path, filename: Option<&str>) -> anyhow::Result<(bool, PathBuf)> {
	let url = link.into_url()?;
	let filename = filename
		.or_else(|| {
			url
			.path_segments()
			.and_then(Iterator::last)
			.and_then(|name| (!name.is_empty()).then_some(name))
		})
		.ok_or_else(|| anyhow!("Filename unknown"))?;

	let (filename, extension) = filename.rsplit_once('.').unwrap_or((filename, "temp"));
	let final_path = path.join(filename).with_extension(extension);
	if !final_path.exists() { fs::create_dir_all(path)?; }

	let file_modify_date = final_path.metadata().and_then(|metadata| metadata.modified()).ok();

	let response = {
		if let Some(date) = file_modify_date {
			let resp = client.get_if_modified_since(url, date).await?;
			if resp.status() == StatusCode::NOT_MODIFIED { return Ok((true, final_path)) }
			else { resp }
		} else { client.get(url).await? }
	};

	let last_modified_header = response.headers().get(header::LAST_MODIFIED).cloned();

	let temp_path = final_path.with_extension("temp");
	let mut writer = File::create(&temp_path)
		.map(BufWriter::new)
		.context(format!("Created file at {:?}", temp_path))?;

	let mut stream = response.bytes_stream();
	while let Some(item) = stream.next().await {
		let chunk = item.context("Error while downloading file")?;
		writer.write_all(&chunk).context("Error writing file")?;
	}
	writer.flush()?;

	fs::rename(&temp_path, &final_path).context(format!("Renamed {:?} to {:?}", temp_path.file_name(), final_path.file_name()))?;

	if let Some(modified) = last_modified_header {
		if let Some(date) = modified.to_str().ok().and_then(|s| parse_http_date(s).ok()) {
			set_file_mtime(&final_path, FileTime::from_system_time(date)).context("Error setting file modified date")?;
		}
	}

	Ok((final_path.exists(), final_path))
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