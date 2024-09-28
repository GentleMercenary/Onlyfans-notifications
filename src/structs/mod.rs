#![allow(dead_code)]

use std::{fs::{self, File}, io::{BufWriter, Write}, path::{Path, PathBuf}};

use anyhow::{anyhow, Context};
use filetime::{set_file_mtime, FileTime};
use futures_util::StreamExt;
use of_client::{client::OFClient, httpdate::parse_http_date, media::{self, MediaType}, user::User, reqwest::{Url, IntoUrl, header, StatusCode}};
use winrt_toast::{Toast, Image, content::image::{ImageHintCrop, ImagePlacement}};

use crate::TEMPDIR;
pub mod socket;


trait ToastExt {
	async fn with_avatar(&mut self, user: &User, client: &OFClient) -> anyhow::Result<&mut Self>;
	async fn with_thumbnail<T: media::Media + Sync>(&mut self, media: &[T], client: &OFClient) -> anyhow::Result<&mut Self>;
}

impl ToastExt for Toast {
	async fn with_avatar(&mut self, user: &User, client: &OFClient) -> anyhow::Result<&mut Self> {
		let user_path = Path::new("data").join(&user.username);
		let mut ret = self;

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

			ret = ret.image(1,
				Image::new_local(avatar.canonicalize()?)?
				.with_hint_crop(ImageHintCrop::Circle)
				.with_placement(ImagePlacement::AppLogoOverride),
			);
		}

		Ok(ret)
	}

	async fn with_thumbnail<T: media::Media + Sync>(&mut self, media: &[T], client: &OFClient) -> anyhow::Result<&mut Self> {
		let thumb = media
			.iter()
			.filter(|media| media.media_type() != &MediaType::Audio)
			.find_map(|media| media.thumbnail().filter(|s| !s.is_empty()));
		
		let mut ret = self;

		if let Some(thumb) = thumb {
			let (_, path) = fetch_file(client, thumb, TEMPDIR.get().unwrap().path(), None).await?;
			ret = ret.image(2, Image::new_local(path)?);
		}

		Ok(ret)
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