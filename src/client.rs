use super::deserializers::{non_empty_string, parse_cookie};
use super::structs::{User, content::PostContent};

use async_trait::async_trait;
use cached::proc_macro::once;
use anyhow::{anyhow, Context};
use futures::{StreamExt, TryFutureExt};
use crypto::{digest::Digest, sha1::Sha1};
use reqwest::{cookie::Jar, header, Client, Response, Url};
use serde::Deserialize;
use std::{
	fmt, fs,
	fs::File,
	io::Write,
	path::{Path, PathBuf},
	sync::Arc,
	time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio_retry::{strategy::ExponentialBackoff, Retry};

#[derive(Clone, Debug)]
pub struct Cookie {
	pub auth_id: String,
	pub sess: String,
	pub auth_hash: String,
}

impl fmt::Display for Cookie {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		f.write_str(&format!("auth_id={}; ", self.auth_id))?;
		f.write_str(&format!("sess={}; ", self.sess))?;
		f.write_str(&format!("auth_hash={}; ", self.auth_hash))?;

		Ok(())
	}
}

#[derive(Deserialize, Clone, Debug)]
struct StaticParams {
	static_param: String,
	format: String,
	checksum_indexes: Vec<i32>,
	checksum_constant: i32,
	app_token: String,
	remove_headers: Vec<String>,
}

#[derive(Deserialize, Clone, Debug)]
struct _AuthParams {
	auth: AuthParams,
}

#[derive(Deserialize, Clone, Debug)]
struct AuthParams {
	#[serde(deserialize_with = "parse_cookie")]
	cookie: Cookie,
	#[serde(deserialize_with = "non_empty_string")]
	x_bc: String,
	#[serde(deserialize_with = "non_empty_string")]
	user_agent: String,
}

#[once(time = 10, result = true, sync_writes = true)]
async fn get_params() -> anyhow::Result<(StaticParams, AuthParams)> {
	Ok((
		reqwest::get("https://raw.githubusercontent.com/DIGITALCRIMINALS/dynamic-rules/main/onlyfans.json")
			.inspect_err(|err| error!("Error getting dynamic rules: {err:?}"))
			.and_then(Response::json::<StaticParams>)
			.await
			.inspect_err(|err| error!("Error reading dynamic rules: {err:?}"))?,
		fs::read_to_string("auth.json")
			.inspect_err(|err| error!("Error reading auth file: {err:?}"))
			.and_then(|data| Ok(serde_json::from_str::<_AuthParams>(&data)?))
			.inspect_err(|err| error!("Error reading auth data: {err:?}"))
			.map(|outer| outer.auth)
			.inspect(|params| debug!("{params:?}"))?,
	))
}

pub struct Unauthorized;
pub struct Authorized { client: Client }

#[async_trait]
pub trait UnauthedClient {
	fn new() -> Self;
	async fn authorize(self) -> anyhow::Result<OFClient<Authorized>>;
}

#[async_trait] 
pub trait AuthedClient {
	async fn make_headers(link: &str) -> anyhow::Result<header::HeaderMap>;
	async fn fetch(&self, link: &str) -> anyhow::Result<Response>;
	async fn fetch_user(&self, user_id: &(impl fmt::Display + std::marker::Sync)) -> anyhow::Result<User>;
	async fn fetch_post(&self, post_id: &(impl fmt::Display + std::marker::Sync)) -> anyhow::Result<PostContent>;
	async fn fetch_file(&self, url: &str, path: &Path, filename: Option<&str>) -> anyhow::Result<(bool, PathBuf)>;
}

pub struct OFClient<Authorization> {
	auth: Authorization,
}

#[async_trait]
impl UnauthedClient for OFClient<Unauthorized> {
	fn new() -> Self {
		Self { auth: Unauthorized }
	}

	async fn authorize(self) -> anyhow::Result<OFClient<Authorized>> {
		let (static_params, auth_params) = get_params().await?;

		let cookie_jar = Jar::default();
		let cookie = &auth_params.cookie;
		let url: Url = "https://onlyfans.com".parse()?;

		cookie_jar.add_cookie_str(&format!("auth_id={}", &cookie.auth_id), &url);
		cookie_jar.add_cookie_str(&format!("sess={}", &cookie.sess), &url);
		cookie_jar.add_cookie_str(&format!("auth_hash={}", &cookie.auth_hash), &url);

		let mut headers = header::HeaderMap::new();
		headers.insert(header::USER_AGENT, header::HeaderValue::from_str(&auth_params.user_agent)?);
		headers.insert("x-bc", header::HeaderValue::from_str(&auth_params.x_bc)?);
		headers.insert("user-id", header::HeaderValue::from_str(&auth_params.cookie.auth_id)?);

		for remove_header in static_params.remove_headers {
			headers.remove(remove_header);
		}

		headers.insert("app-token", header::HeaderValue::from_str(&static_params.app_token)?);

		let client = reqwest::Client::builder()
		.cookie_store(true)
		.cookie_provider(Arc::new(cookie_jar))
		.gzip(true)
		.timeout(Duration::from_secs(30))
		.default_headers(headers)
		.build()?;

		Ok(OFClient {
			auth: Authorized { client }
		})
	}
}

#[async_trait]
impl AuthedClient for OFClient<Authorized> {
	async fn make_headers(link: &str) -> anyhow::Result<header::HeaderMap> {
		let (static_params, auth_params) = get_params().await?;

		let mut headers = header::HeaderMap::new();
		let parsed_url = Url::parse(link)?;
		let mut path = parsed_url.path().to_owned();
		let query = parsed_url.query();

		let mut auth_id = "0";
		if let Some(q) = query {
			auth_id = &auth_params.cookie.auth_id;
			path = format!("{path}?{q}");
			headers.insert("user-id", header::HeaderValue::from_str(auth_id)?);
		}

		let time = SystemTime::now()
			.duration_since(UNIX_EPOCH)?
			.as_secs()
			.to_string();

		let msg = [&static_params.static_param, &time, &path, auth_id].join("\n");
		let mut hasher = Sha1::new();
		hasher.input_str(&msg);

		let sha1_sign = hasher.result_str();
		let sha1_b = sha1_sign.as_bytes();

		let checksum = static_params
			.checksum_indexes
			.iter()
			.map(|x| sha1_b[*x as usize] as i32)
			.sum::<i32>() + static_params.checksum_constant;

		headers.insert("sign", header::HeaderValue::from_str(
				&static_params.format
					.replacen("{}", &sha1_sign, 1)
					.replacen("{:x}", &format!("{:x}", checksum.abs()), 1)
			)?,
		);
		headers.insert("time", header::HeaderValue::from_str(&time)?);

		Ok(headers)
	}

	async fn fetch(&self, link: &str) -> anyhow::Result<Response> {
		let ifetch = || async move {
			let headers = OFClient::make_headers(link).await?;
	
			self.auth.client.get(link)
			.header("accept", "application/json, text/plain, */*")
			.header("connection", "keep-alive")
			.headers(headers)
			.send()
			.await
			.and_then(Response::error_for_status)
			.inspect_err(|err| error!("Error fetching {link}: {err:?}"))
			.map_err(Into::into)
		};

		Retry::spawn(ExponentialBackoff::from_millis(2000).take(5), ifetch).await
	}

	async fn fetch_user(&self, user_id: &(impl fmt::Display + std::marker::Sync)) -> anyhow::Result<User> {
		self.fetch(&format!("https://onlyfans.com/api2/v2/users/{user_id}"))
		.and_then(|response| response.json::<User>().map_err(Into::into))
		.await
		.inspect(|user| info!("Got user: {:?}", user))
		.inspect_err(|err| error!("Error reading user {user_id}: {err:?}"))
	}

	async fn fetch_post(&self, post_id: &(impl fmt::Display + std::marker::Sync)) -> anyhow::Result<PostContent> {
		self.fetch(&format!("https://onlyfans.com/api2/v2/posts/{post_id}"))
		.and_then(|response| response.json::<PostContent>().map_err(Into::into))
		.await
		.inspect(|content| info!("Got content: {:?}", content))
		.inspect_err(|err| error!("Error reading content {post_id}: {err:?}"))
	}

	async fn fetch_file(&self, url: &str, path: &Path, filename: Option<&str>) -> anyhow::Result<(bool, PathBuf)> {
		let parsed_url: Url = url.parse()?;
		let full = filename
			.or_else(|| {
				parsed_url
				.path_segments()
				.and_then(Iterator::last)
				.and_then(|name| name.is_empty().then_some(name))
			})
			.ok_or_else(|| anyhow!("Filename unknown"))?;

		let (filename, _) = full.rsplit_once('.').unwrap();

		let full_path = path.join(full);

		if !full_path.exists() {
			fs::create_dir_all(path)?;
			let temp_path = path.join(filename.to_owned() + ".part");
			let mut f = File::create(&temp_path)?;

			self.fetch(url)
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
			.and_then(|_| Ok(fs::rename(&temp_path, &full_path)?))
			.inspect_err(|err| error!("Error renaming file: {err:?}"))?;
		}

		Ok((full_path.exists(), full_path))
	}
}