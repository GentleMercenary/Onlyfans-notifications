use httpdate::fmt_http_date;
use reqwest_cookie_store::{CookieStore, CookieStoreRwLock};
use serde::{Deserialize, Serialize};
use cached::proc_macro::once;
use futures::TryFutureExt;
use crypto::{digest::Digest, sha1::Sha1};
use reqwest::{header::{self, HeaderValue}, Client, IntoUrl, Method, RequestBuilder, Response, Url};
use std::{borrow::Cow, sync::Arc, time::{SystemTime, UNIX_EPOCH}};

pub struct AuthParams {
	pub cookie: CookieStore,
	pub user_id: String,
	pub x_bc: String,
	pub user_agent: String,
}

#[derive(Deserialize, Debug, Clone)]
struct DynamicRules {
	app_token: String,
	static_param: String,
	prefix: String,
	suffix: String,
	checksum_constant: i32,
	checksum_indexes: Vec<usize>,
}

#[once(time = 3600, result = true, sync_writes = true)]
async fn get_dynamic_rules() -> reqwest::Result<DynamicRules> {
	reqwest::get("https://raw.githubusercontent.com/deviint/onlyfans-dynamic-rules/main/dynamicRules.json")
	.inspect_err(|err| error!("Error getting dynamic rules: {err:?}"))
	.and_then(Response::json::<DynamicRules>)
	.await
	.inspect_err(|err| error!("Error reading dynamic rules: {err:?}"))
}

#[derive(Debug, Clone)]
struct RequestHeaders {
	pub cookie: Arc<CookieStoreRwLock>,
	pub user_id: String,
	pub x_bc: String,
	pub user_agent: String,
}

impl From<AuthParams> for RequestHeaders {
	fn from(value: AuthParams) -> Self {
		Self {
			cookie: Arc::new(CookieStoreRwLock::new(value.cookie)),
			user_id: value.user_id,
			user_agent: value.user_agent,
			x_bc: value.x_bc
		}
	}
}

impl RequestHeaders {
	fn set(&mut self, value: AuthParams) {
		self.user_agent = value.user_agent;
		self.user_id = value.user_id;
		self.x_bc = value.x_bc;

		*self.cookie.write().unwrap() = value.cookie;
	}
}

#[derive(Debug, Clone)]
pub struct OFClient {
	client: Client, headers: RequestHeaders,
}

impl OFClient {
	pub fn new(params: AuthParams) -> reqwest::Result<Self> {
		let headers: RequestHeaders = params.into();

		let client = reqwest::Client::builder()
		.cookie_provider(headers.cookie.clone())
		.gzip(true)
		.build()?;

		Ok(OFClient { client, headers })
	}

	pub fn set_auth_params(&mut self, params: AuthParams) {
		self.headers.set(params);
	}

	async fn make_headers<U: IntoUrl>(&self, link: U) -> reqwest::Result<header::HeaderMap> {
		let dynamic_rules = get_dynamic_rules().await?;

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
		hasher.input_str(
			&[
				&dynamic_rules.static_param,
				&time,
				&*url_param,
				&self.headers.user_id
			].join("\n")
		);

		let sha_hash = hasher.result_str();
		let hash_ascii = sha_hash.as_bytes();

		let checksum = dynamic_rules
		.checksum_indexes
		.into_iter()
		.map(|x| hash_ascii[x] as i32)
		.sum::<i32>() + dynamic_rules.checksum_constant;
	
		let mut headers = header::HeaderMap::new();
		headers.insert(header::ACCEPT, HeaderValue::from_static("application/json, text/plain, */*"));
		headers.insert(header::USER_AGENT, HeaderValue::from_str(&self.headers.user_agent).unwrap());
		headers.insert("x-bc", HeaderValue::from_str(&self.headers.x_bc).unwrap());
		headers.insert("user-id", HeaderValue::from_str(&self.headers.user_id).unwrap());
		headers.insert("time", HeaderValue::from_str(&time).unwrap());
		headers.insert("app-token", HeaderValue::from_str(&dynamic_rules.app_token).unwrap());
		headers.insert("sign", HeaderValue::from_str(
			&format!("{}:{}:{:x}:{}",
				dynamic_rules.prefix,
				sha_hash,
				checksum.abs(),
				dynamic_rules.suffix
			)
		).unwrap());
		

		Ok(headers)
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

	pub async fn post<U: IntoUrl, T: Serialize>(&self, link: U, body: Option<&T>) -> reqwest::Result<Response> {
		let mut builder = self.request(Method::POST, link).await?;
		if let Some(body) = body { builder = builder.json(body); }

		builder
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