#![allow(dead_code)]

use std::{path::{Path, PathBuf}, fs::{File, self}, io::{BufWriter, Write}};

use async_trait::async_trait;
use anyhow::{anyhow, Context};
use futures_util::{TryFutureExt, StreamExt};
use of_client::{client::OFClient, user::User, Url, IntoUrl, media::{self, MediaType}};
use winrt_toast::{Toast, Image, content::image::{ImageHintCrop, ImagePlacement}};

use crate::TEMPDIR;
pub mod socket;


#[async_trait]
pub trait ToastExt {
    async fn with_avatar(&mut self, user: &User, client: &OFClient) -> anyhow::Result<&mut Self>;
    async fn with_thumbnail<T: media::Media + Sync>(&mut self, media: &[T], client: &OFClient) -> anyhow::Result<&mut Self>;
}

#[async_trait]
impl ToastExt for Toast {
    async fn with_avatar(&mut self, user: &User, client: &OFClient) -> anyhow::Result<&mut Self> {
        let user_path = Path::new("data").join(&user.username);

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

            Ok(self.image(1,
                Image::new_local(avatar.canonicalize()?)?
                .with_hint_crop(ImageHintCrop::Circle)
                .with_placement(ImagePlacement::AppLogoOverride),
            ))
        } else {
            Ok (self)
        }
    }

    async fn with_thumbnail<T: media::Media + Sync>(&mut self, media: &[T], client: &OFClient) -> anyhow::Result<&mut Self> {
        let thumb = media
            .iter()
            .filter(|media| media.media_type() != &MediaType::Audio)
            .find_map(|media| media.thumbnail().filter(|s| !s.is_empty()));

		if let Some(thumb) = thumb {
			let (_, path) = fetch_file(client, thumb, TEMPDIR.get().unwrap().path(), None).await?;
			Ok(self.image(2, Image::new_local(path)?))
		} else {
			Ok(self)
		}
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

	if !final_path.exists() {
		fs::create_dir_all(path)?;
		let temp_path = final_path.with_extension("temp");
		let mut writer = File::create(&temp_path)
			.map(BufWriter::new)
			.context(format!("Created file at {:?}", temp_path))?;

		client.get(url)
		.map_err(Into::into)
		.and_then(|response| async move {
			let mut stream = response.bytes_stream();
			while let Some(item) = stream.next().await {
				let chunk = item.context("Error while downloading file")?;
				writer.write_all(&chunk).context("Error writing file")?;
			}
			writer.flush()?;
			Ok(())
		})
		.await
		.inspect_err(|err| error!("{err:?}"))
		.and_then(|_| fs::rename(&temp_path, &final_path).context(format!("Renamed {:?} to {:?}", temp_path.file_name(), final_path.file_name())))
		.inspect_err(|err| error!("Error renaming file: {err:?}"))?;
	}

	Ok((final_path.exists(), final_path))
}