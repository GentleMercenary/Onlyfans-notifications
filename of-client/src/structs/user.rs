use crate::OFClient;
use std::fmt;
use serde::Deserialize;
use futures_util::TryFutureExt;

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Me {
	pub name: String,
	pub id: u64,
	pub username: String,
	pub ws_auth_token: String,
	pub ws_url: String
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct User {
	pub id: u64,
	pub name: String,
	pub username: String,
	pub avatar: Option<String>,
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

pub trait IDType : fmt::Display {}
impl IDType for &str {}
impl IDType for u64 {}

impl OFClient {
	pub async fn get_user<I: IDType>(&self, user_id: I) -> reqwest::Result<User> {
		self.get(format!("https://onlyfans.com/api2/v2/users/{user_id}"))
		.and_then(|response| response.json::<User>().map_err(Into::into))
		.await
		.inspect(|user| info!("Got user: {:?}", user))
		.inspect_err(|err| error!("Error reading user {user_id}: {err:?}"))
	}

	pub async fn subscribe<I: IDType>(&self, user_id: I) -> reqwest::Result<User> {
		self.post(format!("https://onlyfans.com/api2/v2/users/{user_id}/subscribe"), None::<&[u8]>)
		.and_then(|response| response.json::<User>())
		.await
		.inspect(|user| info!("Got user: {:?}", user))
		.inspect_err(|err| error!("Error reading user {user_id}: {err:?}"))
	}

	pub async fn get_subscriptions(&self) -> reqwest::Result<Vec<User>> {
		let count = self.get("https://onlyfans.com/api2/v2/subscriptions/count/all")
		.and_then(|response| response.json::<Subscriptions>())
		.await
		.inspect_err(|err| error!("Error reading subscribe counts: {err:?}"))
		.map(|counts| counts.subscriptions.all)?;

		self.get(format!("https://onlyfans.com/api2/v2/subscriptions/subscribes?limit={count}&offset=0&type=all"))
		.and_then(|response| response.json::<Vec<User>>())
		.await
	}
}