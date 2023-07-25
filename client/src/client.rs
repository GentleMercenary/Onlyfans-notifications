use crate::deserializers::{parse_cookie, non_empty_string};

use serde::{Deserialize, Serialize};
use cached::proc_macro::once;
use anyhow::{anyhow, Context};
use futures::{StreamExt, TryFutureExt};
use crypto::{digest::Digest, sha1::Sha1};
use reqwest::{cookie::Jar, header::{self, HeaderValue}, Client, Response, Url, Method, RequestBuilder, IntoUrl};
use std::{fs::{self, File}, io::Write, path::{Path, PathBuf}, sync::Arc, time::{SystemTime, UNIX_EPOCH}, collections::HashMap};

#[derive(Debug, Clone)]
pub struct Cookie {
	pub sess: String,
	pub auth_id: String,
	pub other: HashMap<String, String>,
}

impl TryFrom<Cookie> for Jar {
	type Error = anyhow::Error;

	fn try_from(value: Cookie) -> Result<Self, Self::Error> {
		let cookie_jar = Jar::default();
		let url: Url = "https://onlyfans.com".parse()?;

		cookie_jar.add_cookie_str(&format!("sess={}", &value.sess), &url);
		cookie_jar.add_cookie_str(&format!("auth_id={}", &value.auth_id), &url);
		for (k, v) in value.other {
			cookie_jar.add_cookie_str(&format!("{}={}", &k, &v), &url);
		}

		Ok(cookie_jar)
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
struct DyanmicRules {
	app_token: String,
	static_param: String,
	start: String,
	end: String,
	checksum_constant: i32,
	checksum_indexes: Vec<i32>,
}

#[once(time = 3600, result = true)]
async fn get_dynamic_rules() -> anyhow::Result<DyanmicRules> {
	reqwest::get("https://raw.githubusercontent.com/deviint/onlyfans-dynamic-rules/main/dynamicRules.json")
	.inspect_err(|err| error!("Error getting dynamic rules: {err:?}"))
	.and_then(Response::json::<DyanmicRules>)
	.await
	.inspect_err(|err| error!("Error reading dynamic rules: {err:?}"))
	.map_err(Into::into)
}

pub struct Unauthorized;
#[derive(Debug)]
pub struct Authorized { client: Client, params: AuthParams }

#[derive(Debug)]
pub struct OFClient<Authorization = Unauthorized> {
	auth: Authorization,
}

impl OFClient {
	pub fn new() -> Self {
		Self { auth: Unauthorized }
	}
}

impl Default for OFClient {
	fn default() -> Self {
		Self::new()
	}
}

impl OFClient<Unauthorized> {
	pub async fn authorize(self, params: AuthParams) -> anyhow::Result<OFClient<Authorized>> {
		let cookie_jar = Jar::try_from(params.cookie.clone())?;

		let mut headers = header::HeaderMap::new();
		headers.insert(header::ACCEPT, HeaderValue::from_static("application/json, text/plain, */*"));
		headers.insert(header::USER_AGENT, HeaderValue::from_str(&params.user_agent)?);
		headers.insert("x-bc", HeaderValue::from_str(&params.x_bc)?);
		headers.insert("user-id", HeaderValue::from_str(&params.cookie.auth_id)?);

		let client = reqwest::Client::builder()
		.cookie_store(true)
		.cookie_provider(Arc::new(cookie_jar))
		.gzip(true)
		.default_headers(headers)
		.build()?;

		Ok(OFClient {
			auth: Authorized { client, params }
		})
	}
}

impl OFClient<Authorized> {
	pub async fn make_headers<U: IntoUrl>(&self, link: U) -> anyhow::Result<header::HeaderMap> {
		let dynamic_rules = get_dynamic_rules().await?;

		let url: Url = link.into_url()?;
		let mut url_param = url.path().to_string();
		if let Some(query) = url.query() {
			url_param.push('?');
			url_param.push_str(query);
		}
		
		let time = SystemTime::now()
			.duration_since(UNIX_EPOCH)?
			.as_secs()
			.to_string();
		
		let mut hasher = Sha1::new();
		hasher.input_str(&[
			dynamic_rules.static_param.as_str(),
			&time,
			&url_param,
			&self.auth.params.cookie.auth_id
			].join("\n"));

		let sha_hash = hasher.result_str();
		let hash_ascii = sha_hash.as_bytes();

		let checksum = dynamic_rules
		.checksum_indexes
		.iter()
		.map(|x| hash_ascii[*x as usize] as i32)
		.sum::<i32>() + dynamic_rules.checksum_constant;
	
	let mut headers = header::HeaderMap::new();
	headers.insert("sign", HeaderValue::from_str(
			&format!("{}:{}:{:x}:{}",
				dynamic_rules.start,
				sha_hash,
				checksum.abs(),
				dynamic_rules.end
			)
		)?);
		headers.insert("time", HeaderValue::from_str(&time)?);
		headers.insert("app-token", HeaderValue::from_str(&dynamic_rules.app_token)?);

		Ok(headers)
	}

	async fn request<U: IntoUrl>(&self, method: Method, link: U) -> anyhow::Result<RequestBuilder> {
		let headers = self.make_headers(link.as_str()).await?;

		Ok(self.auth.client.request(method, link)
			.headers(headers))
	}

	pub async fn get<U: IntoUrl>(&self, link: U) -> anyhow::Result<Response> {
		self.request(Method::GET, link)
		.await?
		.send()
		.await
		.and_then(Response::error_for_status)
		.map_err(Into::into)
	}

	pub async fn post<U: IntoUrl>(&self, link: U, body: Option<&impl Serialize>) -> anyhow::Result<Response> {
		let mut builder = self.request(Method::POST, link).await?;
		if let Some(body) = body { builder = builder.json(body); }

		builder
		.send()
		.await
		.and_then(Response::error_for_status)
		.map_err(Into::into)
	}

	pub async fn fetch_file<U: IntoUrl>(&self, link: U, path: &Path, filename: Option<&str>) -> anyhow::Result<(bool, PathBuf)> {
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
			let temp_path = path.join(filename).with_extension(".temp");
			let mut f = File::create(&temp_path).context(format!("Created file at {:?}", temp_path))?;

			self.get(url)
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
}