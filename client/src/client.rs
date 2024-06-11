use crate::deserializers::{parse_cookie, non_empty_string};

use serde::{Deserialize, Serialize};
use cached::proc_macro::once;
use futures::TryFutureExt;
use crypto::{digest::Digest, sha1::Sha1};
use reqwest::{cookie::Jar, header::{self, HeaderValue}, Client, Response, Url, Method, RequestBuilder, IntoUrl};
use std::{sync::Arc, time::{SystemTime, UNIX_EPOCH}, collections::HashMap};

#[derive(Debug, Clone)]
pub struct Cookie {
	pub sess: String,
	pub auth_id: String,
	pub other: HashMap<String, String>,
}

impl From<&Cookie> for Jar {
	fn from(value: &Cookie) -> Self {
		let cookie_jar = Jar::default();
		let url: Url = "https://onlyfans.com".parse().unwrap();

		cookie_jar.add_cookie_str(&format!("sess={}", &value.sess), &url);
		cookie_jar.add_cookie_str(&format!("auth_id={}", &value.auth_id), &url);
		for (k, v) in &value.other {
			cookie_jar.add_cookie_str(&format!("{}={}", &k, &v), &url);
		}

		cookie_jar
	}
}

#[derive(Deserialize, Debug)]
pub struct AuthParams {
	#[serde(deserialize_with = "parse_cookie")]
	cookie: Cookie,
	#[serde(deserialize_with = "non_empty_string")]
	x_bc: String,
	#[serde(deserialize_with = "non_empty_string")]
	user_agent: String,
}

#[derive(Deserialize, Debug, Clone)]
struct DynamicRules {
	#[serde(rename(deserialize = "app-token"))]
	app_token: String,
	static_param: String,
	prefix: String,
	suffix: String,
	checksum_constant: i32,
	checksum_indexes: Vec<usize>,
}

#[once(time = 3600, result = true)]
async fn get_dynamic_rules() -> reqwest::Result<DynamicRules> {
	reqwest::get("https://raw.githubusercontent.com/Growik/onlyfans-dynamic-rules/main/rules.json")
	.inspect_err(|err| error!("Error getting dynamic rules: {err:?}"))
	.and_then(Response::json::<DynamicRules>)
	.await
	.inspect_err(|err| error!("Error reading dynamic rules: {err:?}"))
}

#[derive(Debug)]
pub struct OFClient {
	client: Client, params: AuthParams,
}

impl OFClient {
	pub async fn new(params: AuthParams) -> reqwest::Result<Self> {
		let cookie_jar = Jar::from(&params.cookie);

		let mut headers = header::HeaderMap::new();
		headers.insert(header::ACCEPT, HeaderValue::from_static("application/json, text/plain, */*"));
		headers.insert(header::USER_AGENT, HeaderValue::from_str(&params.user_agent).unwrap());
		headers.insert("x-bc", HeaderValue::from_str(&params.x_bc).unwrap());
		headers.insert("user-id", HeaderValue::from_str(&params.cookie.auth_id).unwrap());

		let client = reqwest::Client::builder()
		.cookie_store(true)
		.cookie_provider(Arc::new(cookie_jar))
		.gzip(true)
		.default_headers(headers)
		.build()?;

		Ok(OFClient { client, params })
	}

	pub async fn make_headers<U: IntoUrl>(&self, link: U) -> reqwest::Result<header::HeaderMap> {
		let dynamic_rules = get_dynamic_rules().await?;

		let url: Url = link.into_url()?;
		let mut url_param = url.path().to_string();
		if let Some(query) = url.query() {
			url_param.push('?');
			url_param.push_str(query);
		}
		
		let time = SystemTime::now()
			.duration_since(UNIX_EPOCH)
			.unwrap()
			.as_secs()
			.to_string();
		
		let mut hasher = Sha1::new();
		hasher.input_str(&[
			dynamic_rules.static_param.as_str(),
			&time,
			&url_param,
			&self.params.cookie.auth_id
			].join("\n"));

		let sha_hash = hasher.result_str();
		let hash_ascii = sha_hash.as_bytes();

		let checksum = dynamic_rules
		.checksum_indexes
		.into_iter()
		.map(|x| hash_ascii[x] as i32)
		.sum::<i32>() + dynamic_rules.checksum_constant;
	
		let mut headers = header::HeaderMap::new();
		headers.insert("sign", HeaderValue::from_str(
			&format!("{}:{}:{:x}:{}",
				dynamic_rules.prefix,
				sha_hash,
				checksum.abs(),
				dynamic_rules.suffix
			)
		).unwrap());
		
		headers.insert("time", HeaderValue::from_str(&time).unwrap());
		headers.insert("app-token", HeaderValue::from_str(&dynamic_rules.app_token).unwrap());

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
	let result = response.error_for_status_ref();
	match result {
		Ok(_) => Ok(response),
		Err(err) => {
			let url = response.url().clone();
			error!("url: {}, status {}, request body: {}", url, response.status(), response.text().await?);
			Err(err)
		},
	}
}