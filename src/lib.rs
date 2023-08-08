#![feature(result_option_inspect)]
#![feature(min_specialization)]
#![feature(let_chains)]

pub mod settings;
pub mod structs;
pub mod deserializers;
pub mod websocket_client;

#[macro_use]
extern crate log;

use of_client::client::AuthParams;
use tokio::sync::RwLock;
use tempdir::TempDir;
use settings::Settings;
use serde::Deserialize;
use winrt_toast::{ToastManager, register};
use std::{fs, path::Path, io::{Error, ErrorKind}, sync::OnceLock};

pub static MANAGER: OnceLock<ToastManager> = OnceLock::new();
pub static SETTINGS: OnceLock<RwLock<Settings>> = OnceLock::new();
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