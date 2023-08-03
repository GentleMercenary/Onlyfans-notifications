#![feature(result_option_inspect)]
#![feature(let_chains)]

pub mod settings;
pub mod structs;
pub mod deserializers;
pub mod websocket_client;

#[macro_use]
extern crate log;

use futures_util::{TryFutureExt, StreamExt};
use of_client::{client::{AuthParams, OFClient, Authorized}, IntoUrl};
use tokio::sync::Mutex;
use tempdir::TempDir;
use settings::Settings;
use anyhow::{anyhow, Context};
use serde::Deserialize;
use winrt_toast::{ToastManager, register};
use std::{fs::{self, File}, path::{Path, PathBuf}, io::{Error, ErrorKind, Write}, sync::OnceLock};

pub static MANAGER: OnceLock<ToastManager> = OnceLock::new();
pub static SETTINGS: OnceLock<Mutex<Settings>> = OnceLock::new();
static TEMPDIR: OnceLock<TempDir> = OnceLock::new();

pub fn init() -> anyhow::Result<()> {
	let aum_id = "OFNotifier";
	let icon_path = Path::new("icons").join("icon.ico").canonicalize()
		.inspect_err(|err| error!("{err}"))?;

	register(aum_id, "OF notifier", Some(icon_path.as_path()))
	.inspect_err(|err| error!("{err}"))?;
	
	let _ = MANAGER
	.set(ToastManager::new(aum_id))
	.inspect_err(|_| error!("toast manager set"));

	TempDir::new("OF_thumbs")
	.and_then(|dir| TEMPDIR.set(dir).map_err(|_| Error::new(ErrorKind::Other, "OnceCell couldn't set")))
	.inspect_err(|err| error!("{err}"))?;

	Ok(())
}

pub fn get_auth_params() -> anyhow::Result<AuthParams> {
	#[derive(Deserialize)]
	struct _AuthParams { auth: AuthParams }

	fs::read_to_string("auth.json")
	.inspect_err(|err| error!("Error reading auth file: {err:?}"))
	.and_then(|data| Ok(serde_json::from_str::<_AuthParams>(&data)?))
	.inspect_err(|err| error!("Error reading auth data: {err:?}"))
	.map(|params| params.auth)
	.inspect(|params| debug!("{params:?}"))
	.map_err(Into::into)
}

pub async fn fetch_file<U: IntoUrl>(client: &OFClient<Authorized>, link: U, path: &Path, filename: Option<&str>) -> anyhow::Result<(bool, PathBuf)> {
	let url = link.into_url()?;
	let filename = filename
	.or_else(|| {
		url
		.path_segments()
		.and_then(Iterator::last)
		.and_then(|name| (!name.is_empty()).then_some(name))
	})
	.ok_or_else(|| anyhow!("Filename unknown"))?;

	let (filename, extension) = filename.rsplit_once('.').context("File has no extension")?;
	let final_path = path.join(filename).with_extension(extension);

	if !final_path.exists() {
		fs::create_dir_all(path)?;
		let temp_path = path.join(filename).with_extension("temp");
		let mut f = File::create(&temp_path).context(format!("Created file at {:?}", temp_path))?;

		client.get(url)
		.map_err(Into::into)
		.and_then(|response| async move {
			let mut stream = response.bytes_stream();
			while let Some(item) = stream.next().await {
				let chunk = item.context("Error while downloading file")?;
				f.write_all(&chunk).context("Error writing file")?;
			}
			Ok(())
		})
		.await
		.inspect_err(|err| error!("{err:?}"))
		.and_then(|_| fs::rename(&temp_path, &final_path).context(format!("Renamed {:?} to {:?}", temp_path.file_name(), final_path.file_name())))
		.inspect_err(|err| error!("Error renaming file: {err:?}"))?;
	}

	Ok((final_path.exists(), final_path))
}