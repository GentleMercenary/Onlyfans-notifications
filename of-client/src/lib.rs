#![feature(once_cell_try)]

#[macro_use]
extern crate log;

pub mod structs;
#[cfg(feature = "drm")]
pub mod drm;
#[cfg(feature = "drm")]
pub use widevine;
mod rules;

pub use reqwest;
pub use reqwest_middleware;
pub use reqwest_cookie_store;
pub use structs::{content, media, user};

use log::*;
use reqwest_cookie_store::CookieStoreRwLock;
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware, Middleware, Next};
use http::Extensions;
use cached::proc_macro::once;
use futures::{future::BoxFuture};
use sha1_smol::Sha1;
use reqwest::{header::{self, HeaderValue}, Request, Response, Url};
use std::{borrow::Cow, ops::Deref, sync::{Arc, OnceLock, RwLock}, time::{SystemTime, UNIX_EPOCH}};

use crate::rules::{DynamicRules, DynamicRulesProvider, RulesError};

#[once(time = 3600, result = true, sync_writes = true)]
async fn get_dynamic_rules() -> Result<DynamicRules, RulesError> {
	static PROVIDER: OnceLock<DynamicRulesProvider> = OnceLock::new();
	let provider = PROVIDER.get_or_try_init(|| { DynamicRulesProvider::new() })?;
	provider.read().await
}

#[derive(Debug)]
pub struct RequestHeaders {
	pub cookie: Arc<CookieStoreRwLock>,
	pub user_id: String,
	pub x_bc: String,
	pub user_agent: String,
}

pub struct SharedRequestHeaders(RwLock<RequestHeaders>);

impl Deref for SharedRequestHeaders {
	type Target = RwLock<RequestHeaders>;
	fn deref(&self) -> &Self::Target { &self.0 }
}

impl SharedRequestHeaders {
	pub fn set<H: Into<RequestHeaders>>(&self, headers: H) {
		*self.write().unwrap() = headers.into();
	}

	fn insert_into(&self, rules: &DynamicRules, req: &mut Request) {
		let params = self.read().unwrap();

		let mut url_param = Cow::Borrowed(req.url().path());
		if let Some(query) = req.url().query() {
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
		hasher.update(rules.static_param.as_bytes());	hasher.update(b"\n");
		hasher.update(time.as_bytes());							hasher.update(b"\n");
		hasher.update(url_param.as_bytes());					hasher.update(b"\n");
		hasher.update(params.user_id.as_bytes());

		let digest = hasher.digest().to_string();
		let digest_bytes = digest.as_bytes();

		let checksum = (rules
			.checksum_indexes
			.iter()
			.map(|x| digest_bytes[*x] as i32)
			.sum::<i32>() + rules.checksum_constant
		).abs();
	
		let header_map = req.headers_mut();
		header_map.insert(header::ACCEPT, HeaderValue::from_static("application/json, text/plain, */*"));
		header_map.insert(header::USER_AGENT, HeaderValue::from_str(&params.user_agent).unwrap());
		header_map.insert("x-bc", HeaderValue::from_str(&params.x_bc).unwrap());
		header_map.insert("user-id", HeaderValue::from_str(&params.user_id).unwrap());
		header_map.insert("time", HeaderValue::from_str(&time).unwrap());
		header_map.insert("app-token", HeaderValue::from_str(&rules.app_token).unwrap());
		header_map.insert("sign", HeaderValue::from_str(&format!("{}:{digest}:{checksum:x}:{}", rules.prefix, rules.suffix)).unwrap());
	}
}

#[async_trait::async_trait]
impl Middleware for SharedRequestHeaders {
	async fn handle(&self, mut req: Request, extensions: &mut Extensions, next: Next<'_>) -> reqwest_middleware::Result<Response> {
		let rules = get_dynamic_rules().await.map_err(|err| reqwest_middleware::Error::Middleware(err.into()))?;
		self.insert_into(&rules, &mut req);
		next.run(req, extensions).await
	}
}

fn status_error_log_middleware<'a>(req: Request, extensions: &'a mut Extensions, next: Next<'a>) -> BoxFuture<'a, reqwest_middleware::Result<Response>> {
	Box::pin(async move {
		let response = next.run(req, extensions).await?;
		match response.error_for_status_ref() {
			Ok(_) => Ok(response),
			Err(err) => {
				error!("url: {:?}, status {}, request body: {}", err.url().map(Url::as_str), response.status(), response.text().await?);
				Err(err.into())
			},
		}
	})
}

#[derive(Clone)]
pub struct OFClient {
	client: ClientWithMiddleware,
	pub headers: Arc<SharedRequestHeaders>
}

impl OFClient {
	pub fn new<H: Into<RequestHeaders>>(headers: H) -> reqwest::Result<Self> {
		let headers = headers.into();
		let cookie = headers.cookie.clone();
		let headers = Arc::new(SharedRequestHeaders(RwLock::new(headers)));

		Ok(Self {
			client: ClientBuilder::new(
				reqwest::Client::builder()
				.cookie_provider(cookie)
				.gzip(true)
				.build()?
			)
			.with_arc(headers.clone())
			.with(status_error_log_middleware)
			.build(),
			headers
		})
	}
}

impl Deref for OFClient {
	type Target = ClientWithMiddleware;
	fn deref(&self) -> &Self::Target { &self.client }
}