use crate::{client::{Authorized, OFClient}, deserializers::de_markdown_string};

use std::fmt;
use serde::Deserialize;
use futures_util::{TryFutureExt, future::try_join_all};

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Me {
	#[serde(deserialize_with = "de_markdown_string")]
	pub name: String,
	pub username: String,
	pub ws_auth_token: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct User {
	pub id: u64,
	pub name: String,
	pub username: String,
	pub avatar: String,
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
	pub async fn get_user(&self, user_id: &(impl fmt::Display + std::marker::Sync)) -> anyhow::Result<User> {
		self.get(&format!("https://onlyfans.com/api2/v2/users/{user_id}"))
		.and_then(|response| response.json::<User>().map_err(Into::into))
		.await
		.inspect(|user| info!("Got user: {:?}", user))
		.inspect_err(|err| error!("Error reading user {user_id}: {err:?}"))
	}

	pub async fn subscribe(&self, user_id: &(impl fmt::Display + std::marker::Sync)) -> anyhow::Result<User> {
		self.post(&format!("https://onlyfans.com/api2/v2/users/{user_id}/subscribe"))
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

#[cfg(test)]
mod tests {
	use futures_util::TryFutureExt;
	use crate::get_auth_params;
	use super::OFClient;

	#[tokio::test]
	async fn get_subscriptions() -> anyhow::Result<()> {
		let client = futures::future::ready(get_auth_params())
			.and_then(|params| OFClient::new().authorize(params))
			.await?;

		let users = client.get_subscriptions().await?;
		for user in users {
			println!("{user:?}");
		}

		Ok(())
	}
}