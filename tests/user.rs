mod init;

use of_notifier::get_auth_params;
use of_client::client::OFClient;
use init::init_log;

#[tokio::test]
async fn get_subscriptions() {
	init_log();

	let params = get_auth_params().unwrap();
	let client = OFClient::new(params).unwrap();

	for user in client.get_subscriptions().await.unwrap() {
		println!("{user:?}");
	}
}

#[tokio::test]
async fn get_user_name() {
	init_log();

	let params = get_auth_params().unwrap();
	let client = OFClient::new(params).unwrap();

	let user = client.get_user("onlyfans").await.unwrap();
	println!("{user:?}");
}

#[tokio::test]
async fn get_user_id() {
	init_log();

	let params = get_auth_params().unwrap();
	let client = OFClient::new(params).unwrap();

	let user = client.get_user(15585607).await.unwrap();
	println!("{user:?}");
}