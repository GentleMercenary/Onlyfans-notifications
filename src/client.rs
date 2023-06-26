use crate::deserializers::{parse_cookie, non_empty_string};

use serde::Deserialize;
use cached::proc_macro::once;
use anyhow::{anyhow, Context};
use futures::{StreamExt, TryFutureExt};
use crypto::{digest::Digest, sha1::Sha1};
use tokio_retry::{strategy::FixedInterval, Retry};
use reqwest::{cookie::Jar, header, Client, Response, Url};
use std::{fs::{self, File}, io::Write, path::{Path, PathBuf}, sync::Arc, time::{SystemTime, UNIX_EPOCH}};

#[derive(Debug)]
pub struct Cookie {
	pub auth_id: String,
	pub sess: String,
	pub auth_hash: String,
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
	#[serde(rename = "app-token")]
	app_token: String,
	static_param: String,
	prefix: String,
	suffix: String,
	checksum_constant: i32,
	checksum_indexes: Vec<i32>
}

#[once(time = 1800, result = true, sync_writes = true)]
async fn get_dynamic_rules() -> anyhow::Result<DyanmicRules> {
	reqwest::get("https://raw.githubusercontent.com/SneakyOvis/onlyfans-dynamic-rules/main/rules.json")
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

impl OFClient<Unauthorized> {
	pub async fn authorize(self, params: AuthParams) -> anyhow::Result<OFClient<Authorized>> {
		let dynamic_rules = get_dynamic_rules().await?;

		let cookie_jar = Jar::default();
		let url: Url = "https://onlyfans.com".parse()?;

		cookie_jar.add_cookie_str(&format!("auth_id={}", &params.cookie.auth_id), &url);
		cookie_jar.add_cookie_str(&format!("sess={}", &params.cookie.sess), &url);
		cookie_jar.add_cookie_str(&format!("auth_hash={}", &params.cookie.auth_hash), &url);

		let mut headers = header::HeaderMap::new();
		headers.insert(header::USER_AGENT, header::HeaderValue::from_str(&params.user_agent)?);
		headers.insert("x-bc", header::HeaderValue::from_str(&params.x_bc)?);
		headers.insert("user-id", header::HeaderValue::from_str(&params.cookie.auth_id)?);
		headers.insert("app-token", header::HeaderValue::from_str(&dynamic_rules.app_token)?);

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
	pub async fn make_headers(&self, link: &str) -> anyhow::Result<header::HeaderMap> {
		let dynamic_rules = get_dynamic_rules().await?;

		let mut headers = header::HeaderMap::new();

		headers.insert("accept", header::HeaderValue::from_static("application/json, text/plain, */*"));

		let parsed_url = Url::parse(link)?;
		let path = parsed_url.path();

		let time = SystemTime::now()
			.duration_since(UNIX_EPOCH)?
			.as_secs()
			.to_string();

		let msg = [&dynamic_rules.static_param, &time, path, &self.auth.params.cookie.auth_id].join("\n");
		let mut hasher = Sha1::new();
		hasher.input_str(&msg);

		let sha_hash = hasher.result_str();
		let hash_ascii = sha_hash.as_bytes();

		let checksum = dynamic_rules
			.checksum_indexes
			.iter()
			.map(|x| hash_ascii[*x as usize] as i32)
			.sum::<i32>() + dynamic_rules.checksum_constant;

		headers.insert("sign", header::HeaderValue::from_str(
			&format!("{}:{}:{:x}:{}",
				dynamic_rules.prefix,
				sha_hash,
				checksum.abs(),
				dynamic_rules.suffix
			)
		)?);
		headers.insert("time", header::HeaderValue::from_str(&time)?);

		Ok(headers)
	}

	pub async fn get(&self, link: &str) -> anyhow::Result<Response> {
		let iget = || async move {
			let headers = self.make_headers(link).await?;

			self.auth.client.get(link)
			.headers(headers)
			.send()
			.await
			.and_then(Response::error_for_status)
			.map_err(Into::into)
		};

		Retry::spawn(FixedInterval::from_millis(1000).take(5), iget).await
	}

	pub async fn post(&self, link: &str) -> anyhow::Result<Response> {
		let ipost = || async move {
			let headers = self.make_headers(link).await?;
	
			self.auth.client.post(link)
			.headers(headers)
			.send()
			.await
			.and_then(Response::error_for_status)
			.map_err(Into::into)
		};

		Retry::spawn(FixedInterval::from_millis(1000).take(5), ipost).await
	}

	pub async fn fetch_file(&self, url: &str, path: &Path, filename: Option<&str>) -> anyhow::Result<(bool, PathBuf)> {
		let parsed_url: Url = url.parse()?;
		let full = filename
		.or_else(|| {
			parsed_url
			.path_segments()
			.and_then(Iterator::last)
			.and_then(|name| (!name.is_empty()).then_some(name))
		})
		.ok_or_else(|| anyhow!("Filename unknown"))?;
	
	let (filename, _) = full.rsplit_once('.').expect("Split extension from filename");
	
	let full_path = path.join(full);
	
		if !full_path.exists() {
			fs::create_dir_all(path)?;
			let temp_path = path.join(filename).with_extension(".path");
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
			.and_then(|_| fs::rename(&temp_path, &full_path).context(format!("Renamed {:?} to {:?}", temp_path, full_path)))
			.inspect_err(|err| error!("Error renaming file: {err:?}"))?;
		}

		Ok((full_path.exists(), full_path))
	}
}