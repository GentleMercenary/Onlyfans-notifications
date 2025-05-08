#[macro_use]
extern crate log;

pub mod structs;
#[cfg(feature = "drm")]
pub mod drm;
#[cfg(feature = "drm")]
pub use widevine;

pub use reqwest;
pub use reqwest_cookie_store;
pub use httpdate;
pub use structs::{content, media, user};

use log::*;
use httpdate::fmt_http_date;
use reqwest_cookie_store::CookieStoreRwLock;
use serde::{Deserialize, Serialize};
use cached::proc_macro::once;
use futures::TryFutureExt;
use sha1_smol::Sha1;
use reqwest::{header::{self, HeaderValue}, Body, Client, IntoUrl, Method, RequestBuilder, Response, Url};
use std::{borrow::Cow, sync::{Arc, RwLock}, time::{SystemTime, UNIX_EPOCH}};

#[derive(Deserialize, Debug, Clone)]
struct DynamicRules {
	#[serde(rename = "app-token")]
	app_token: String,
	static_param: String,
	prefix: String,
	suffix: String,
	checksum_constant: i32,
	checksum_indexes: Vec<usize>,
}

#[once(time = 3600, result = true, sync_writes = true)]
async fn get_dynamic_rules() -> reqwest::Result<DynamicRules> {
	reqwest::get("https://raw.githubusercontent.com/rafa-9/dynamic-rules/refs/heads/main/rules.json")
	.and_then(Response::json::<DynamicRules>)
	.await
	.inspect_err(|err| error!("Error reading dynamic rules: {err:?}"))
}

#[derive(Debug)]
pub struct RequestHeaders {
	pub cookie: Arc<CookieStoreRwLock>,
	pub user_id: String,
	pub x_bc: String,
	pub user_agent: String,
}

#[derive(Debug, Clone)]
pub struct OFClient {
	client: Client,
	pub headers: Arc<RwLock<RequestHeaders>>,
}

impl OFClient {
	pub fn new<H: Into<RequestHeaders>>(headers: H) -> reqwest::Result<Self> {
		let headers = headers.into();

		let client = reqwest::Client::builder()
		.cookie_provider(headers.cookie.clone())
		.gzip(true)
		.build()?;

		Ok(OFClient { client, headers: Arc::new(RwLock::new(headers)) })
	}

	async fn make_headers<U: IntoUrl>(&self, link: U) -> reqwest::Result<header::HeaderMap> {
		let dynamic_rules = get_dynamic_rules().await?;
		let headers = self.headers.read().unwrap();

		let url: Url = link.into_url()?;
		let mut url_param: Cow<'_, str> = Cow::Borrowed(url.path());
		if let Some(query) = url.query() {
			let mut s = url_param.into_owned();
			s.push('?');
			s.push_str(query);
			url_param = Cow::Owned(s);
		}
		
		let time = SystemTime::now()
			.duration_since(UNIX_EPOCH)
			.unwrap()
			.as_secs()
			.to_string();
		
		let mut hasher = Sha1::new();
		hasher.update(dynamic_rules.static_param.as_bytes());	hasher.update(b"\n");
		hasher.update(time.as_bytes());							hasher.update(b"\n");
		hasher.update(url_param.as_bytes());					hasher.update(b"\n");
		hasher.update(headers.user_id.as_bytes());

		let digest = hasher.digest().to_string();
		let digest_bytes = digest.as_bytes();

		let checksum = dynamic_rules
		.checksum_indexes
		.into_iter()
		.map(|x| digest_bytes[x] as i32)
		.sum::<i32>() + dynamic_rules.checksum_constant;
	
		let mut header_map = header::HeaderMap::new();
		header_map.insert(header::ACCEPT, HeaderValue::from_static("application/json, text/plain, */*"));
		header_map.insert(header::USER_AGENT, HeaderValue::from_str(&headers.user_agent).unwrap());
		header_map.insert("x-bc", HeaderValue::from_str(&headers.x_bc).unwrap());
		header_map.insert("user-id", HeaderValue::from_str(&headers.user_id).unwrap());
		header_map.insert("time", HeaderValue::from_str(&time).unwrap());
		header_map.insert("app-token", HeaderValue::from_str(&dynamic_rules.app_token).unwrap());
		header_map.insert("sign", HeaderValue::from_str(
			&format!("{}:{}:{:x}:{}",
				dynamic_rules.prefix,
				digest,
				checksum.abs(),
				dynamic_rules.suffix
			)
		).unwrap());
		
		Ok(header_map)
	}

	async fn request<U: IntoUrl>(&self, method: Method, link: U) -> reqwest::Result<RequestBuilder> {
		let headers = self.make_headers(link.as_str()).await?;

		Ok(self.client.request(method, link)
			.headers(headers))
	}

	pub async fn get<U: IntoUrl>(&self, link: U) -> reqwest::Result<Response> {
		self.request(Method::GET, link)
		.await?
		.send()
		.and_then(error_for_status_log)
		.await
	}

	pub async fn get_if_modified_since<U: IntoUrl>(&self, link: U, modified_date: SystemTime) -> reqwest::Result<Response> {
		self.request(Method::GET, link).await?
		.header(header::IF_MODIFIED_SINCE, HeaderValue::from_str(&fmt_http_date(modified_date)).unwrap())
		.send()
		.and_then(error_for_status_log)
		.await
	}

	pub async fn post<U: IntoUrl, T: Into<Body>>(&self, link: U, body: Option<T>) -> reqwest::Result<Response> {
		let mut builder = self.request(Method::POST, link).await?;
		if let Some(body) = body { builder = builder.body(body); }

		builder
		.send()
		.and_then(error_for_status_log)
		.await
	}

	pub async fn post_json<U: IntoUrl, T: Serialize>(&self, link: U, body: &T) -> reqwest::Result<Response> {
		self.request(Method::POST, link).await?
		.json(body)
		.send()
		.and_then(error_for_status_log)
		.await
	}

	pub async fn put<U: IntoUrl, T: Serialize>(&self, link: U, body: Option<&T>) -> reqwest::Result<Response> {
		let mut builder = self.request(Method::PUT, link).await?;
		if let Some(body) = body { builder = builder.json(body); }

		builder
		.send()
		.and_then(error_for_status_log)
		.await
	}
}

async fn error_for_status_log(response: Response) -> reqwest::Result<Response> {
	match response.error_for_status_ref() {
		Ok(_) => Ok(response),
		Err(err) => {
			error!("url: {:?}, status {}, request body: {}", err.url(), response.status(), response.text().await?);
			Err(err)
		},
	}
}