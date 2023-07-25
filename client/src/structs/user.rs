use crate::{client::{Authorized, OFClient}, deserializers::de_markdown_string};

use std::{fmt, path::{PathBuf, Path}};
use reqwest::Url;
use serde::Deserialize;
use futures_util::{TryFutureExt, future::try_join_all};
use anyhow::anyhow;

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Me {
	#[serde(deserialize_with = "de_markdown_string")]
	pub name: String,
	pub id: u64,
	pub username: String,
	pub ws_auth_token: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct User {
	pub id: u64,
	pub name: String,
	pub username: String,
	pub avatar: Option<String>,
}

impl User {
	pub async fn download_avatar(&self, client: &OFClient<Authorized>) -> anyhow::Result<Option<PathBuf>> {
		if let Some(avatar) = &self.avatar {
			let avatar_url = avatar.parse::<Url>()?;
			let filename = avatar_url
				.path_segments()
				.and_then(|segments| {
					let mut reverse_iter = segments.rev();
					let ext = reverse_iter.next().and_then(|file| file.split('.').last());
					let filename = reverse_iter.next();
		
					Option::zip(filename, ext).map(|(filename, ext)| [filename, ext].join("."))
				})
				.ok_or_else(|| anyhow!("Filename unknown"))?;
		
			let mut user_path = Path::new("data").join(&self.username);
			std::fs::create_dir_all(&user_path)?;
			user_path = user_path.canonicalize()?;
		
			let (_, avatar) = client.fetch_file(
					avatar,
					&user_path.join("Profile").join("Avatars"),
					Some(&filename),
				)
				.await?;
			Ok(Some(avatar))
		} else { Ok(None) }
	}
}

#[derive(Deserialize, Debug)]
pub struct SubscriberCategories {
	pub active: u32,
	pub muted: u32,
	pub restricted: u32,
	pub expired: u32,
	pub blocked: u32,
	pub all: u32
	// pub attention: u32, // only for subscriptions
	// activeOnline: u32, // only for subscribers
}

#[derive(Deserialize, Debug)]
pub struct Subscriptions {
	pub subscriptions: SubscriberCategories,
	pub subscribers: SubscriberCategories,
	pub bookmarks: u32,
}

impl OFClient<Authorized> {
	pub async fn get_user<S: fmt::Display>(&self, user_id: S) -> anyhow::Result<User> {
		self.get(&format!("https://onlyfans.com/api2/v2/users/{user_id}"))
		.and_then(|response| response.json::<User>().map_err(Into::into))
		.await
		.inspect(|user| info!("Got user: {:?}", user))
		.inspect_err(|err| error!("Error reading user {user_id}: {err:?}"))
	}

	pub async fn subscribe<S: fmt::Display>(&self, user_id: S) -> anyhow::Result<User> {
		self.post(&format!("https://onlyfans.com/api2/v2/users/{user_id}/subscribe"), None as Option<&String>)
		.and_then(|response| response.json::<User>().map_err(Into::into))
		.await
		.inspect(|user| info!("Got user: {:?}", user))
		.inspect_err(|err| error!("Error reading user {user_id}: {err:?}"))
	}

	pub async fn get_subscriptions(&self) -> anyhow::Result<Vec<User>> {
		let count = self.get("https://onlyfans.com/api2/v2/subscriptions/count/all")
		.and_then(|response| response.json::<Subscriptions>().map_err(Into::into))
		.await
		.inspect_err(|err| error!("Error reading subscribe counts: {err:?}"))
		.map(|counts| counts.subscriptions.all)?;
	
		const LIMIT: i32 = 10;
		let n = (count as f32 / LIMIT as f32).ceil() as i32;

		try_join_all(
			(0..n+1)
			.map(|a| async move {
				let offset = a * LIMIT;
				self.get(&format!("https://onlyfans.com/api2/v2/subscriptions/subscribes?limit={LIMIT}&offset={offset}&type=all"))
				.and_then(|response| response.json::<Vec<User>>().map_err(Into::into))
				.await
			})
		).await
		.map(|i| i.into_iter().flatten().collect())
	}
}