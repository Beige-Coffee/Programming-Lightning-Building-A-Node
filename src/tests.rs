use super::*;
use crate::bitcoind_client::BitcoindClient;
use crate::commands::{
	open_channel, send_payment, HTLCStatus, MillisatAmount as SatAmount,
	OutboundPaymentInfoStorage, PaymentInfo,
};
use crate::events::handle_ldk_events;
use crate::hex_utils;
use crate::internal::bitcoind_client::BitcoindClient as BitcoinClient;
use crate::internal::channel_manager::ChannelManager;
use crate::internal::types::{FileStore, KeysManager, PeerManager};
use crate::internal::types::OnChainWallet as MockOnChainWallet;
use crate::keys_manager::NodeKeysManager;
use crate::logger::FilesystemLogger;
use crate::networking::start_network_listener;
use crate::networking::MockPeerManager;
use crate::LdkOnChainWallet;
use bitcoin::consensus::encode::{deserialize, serialize_hex};
use bitcoin::consensus::{encode, Decodable, Encodable};
use bitcoin::hash_types::Txid;
use bitcoin::script::Builder;
use bitcoin::secp256k1::SecretKey;
use bitcoin::secp256k1::{self, PublicKey, Secp256k1};
use bitcoin::Network;
use bitcoin::ScriptBuf;
use chrono::Utc;
use filesystem_store::FilesystemStore as ExerciseFileStore;
use hex_lit::hex;
use lightning::chain::chaininterface::{BroadcasterInterface, ConfirmationTarget, FeeEstimator};
use lightning::events::Event;
use lightning::io::ErrorKind;
use lightning::ln::types::ChannelId;
use lightning::util::logger::{Level, Logger, Record};
use lightning_block_sync::{AsyncBlockSourceResult, BlockData, BlockHeaderData, BlockSource};
use lightning_invoice::Bolt11Invoice;
use std::collections::HashMap;
use std::env::temp_dir;
use std::io::Read;
use std::path::Path;
use std::str::FromStr;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::time::{timeout, Duration};

async fn get_bitcoind_client() -> BitcoindClient {
	let logger = Arc::new(FilesystemLogger::new("test_dir".to_string()));

	let args = args::get_config_info();

	let client = BitcoindClient::new(
		args.bitcoind_rpc_host.clone(),
		args.bitcoind_rpc_port,
		args.bitcoind_rpc_username.clone(),
		args.bitcoind_rpc_password.clone(),
		args.network,
		tokio::runtime::Handle::current(),
		Arc::clone(&logger),
	)
	.await
	.unwrap();

	client
}

async fn get_wallet() -> Arc<LdkOnChainWallet> {
	let args = args::get_config_info();

	let logger = Arc::new(FilesystemLogger::new("test_dir".to_string()));

	let keys_seed_path = format!("{}/keys_seed", "test_dir");
	let keys_seed = if let Ok(seed) = fs::read(keys_seed_path.clone()) {
		assert_eq!(seed.len(), 32);
		let mut key = [0; 32];
		key.copy_from_slice(&seed);
		key
	} else {
		let mut key = [0; 32];
		thread_rng().fill_bytes(&mut key);
		match File::create(keys_seed_path.clone()) {
			Ok(mut f) => {
				std::io::Write::write_all(&mut f, &key)
					.expect("Failed to write node keys seed to disk");
				f.sync_all().expect("Failed to sync node keys seed to disk");
			},
			Err(e) => {
				println!("ERROR: Unable to create keys seed file {}: {}", keys_seed_path, e);
			},
		}
		key
	};

	let bitcoind_client = Arc::new(get_bitcoind_client().await);
	let network = Network::Regtest;
	let on_chain_wallet_file_path = "./test_dir/test_wallet.sqlite3";

	let on_chain_wallet = Arc::new(OnChainWallet::new_from_seed(
		&keys_seed,
		args.network.clone(),
		on_chain_wallet_file_path,
		bitcoind_client.clone(),
		bitcoind_client.clone(),
		Arc::clone(&logger),
	));

	let address = on_chain_wallet.get_address();
	//println!("Test On Chain Wallet Address: {:?}", address);

	let balance = on_chain_wallet.get_balance();
	//println!("Test On Chain Wallet Balance: {:?}", balance);

	on_chain_wallet
}

#[cfg(test)]
mod bitcoind_tests {
	use super::*;


	#[test]
	fn test_01_filesystem_logger_new() {
		// Test 2: Verify FilesystemLogger::new implementation
		let temp_dir = temp_dir();
		let data_dir = temp_dir.to_string_lossy().to_string();

		let logger = FilesystemLogger::new(data_dir.clone());

		// Verify the logs directory path
		let expected_path = format!("{}/logs", data_dir);
		assert_eq!(logger.data_dir, expected_path);

		// Verify directory exists and is a directory
		let metadata = fs::metadata(&expected_path).unwrap();
		assert!(metadata.is_dir(), "Logs path is not a directory");

		// Verify we can create nested directories
		let nested_dir = temp_dir.to_string_lossy().to_string();
		let nested_logger = FilesystemLogger::new(nested_dir.clone());
		let nested_path = format!("{}/logs", nested_dir);
		assert_eq!(nested_logger.data_dir, nested_path);
		assert!(fs::metadata(&nested_path).is_ok(), "Nested logs directory was not created");
	}

	#[test]
	fn test_02_filesystem_logger_log() {
		// Test 3: Verify Logger trait implementation
		let temp_dir = temp_dir();
		let data_dir = temp_dir.to_string_lossy().to_string();
		let logger = FilesystemLogger::new(data_dir.clone());

		// Create a test record using LDK's Record
		let record = Record {
			level: Level::Info,
			peer_id: None,
			channel_id: None,
			args: format_args!("Test log message"),
			module_path: "test_module",
			file: "test_file.rs",
			line: 42,
			payment_hash: None,
		};

		// Log the record
		logger.log(record);

		// Read the log file
		let log_file_path = format!("{}/logs/logs.txt", data_dir);
		let mut file = File::open(&log_file_path).unwrap();
		let mut contents = String::new();
		file.read_to_string(&mut contents).unwrap();

		// Verify log format
		let current_time = Utc::now();
		let expected_time_format = current_time.format("%Y-%m-%d %H:%M:%S%.3f").to_string();
		let expected_log =
			format!("{} {:<5} [test_module:42] Test log message\n", expected_time_format, "INFO");

		// Since exact timestamp might vary slightly, check key components
		assert!(contents.contains("INFO"), "Log level not found");
		assert!(contents.contains("[test_module:42]"), "Module and line not found");
		assert!(contents.contains("Test log message"), "Log message not found");

		// Verify timestamp format (basic check for YYYY-MM-DD HH:MM:SS.fff)
		assert!(
			contents.contains(&current_time.format("%Y-%m-%d").to_string()),
			"Date format incorrect"
		);
	}

	#[tokio::test]
	async fn test_03_bitcoind_client_new() {
		let client = get_bitcoind_client().await;
		let blockchain_info = client.get_blockchain_info().await.chain;

		assert_eq!(blockchain_info, "regtest");
	}

	#[tokio::test]
	async fn test_04_block_source() {
		let client = get_bitcoind_client().await;

		let best_block_user = client.get_best_block().await.unwrap().0;

		let block_user = client.get_block(&best_block_user).await.unwrap();

		let header_user = match block_user {
			BlockData::HeaderOnly(header) => header,
			BlockData::FullBlock(block) => block.header,
		};

		let best_header_user = client.get_header(&best_block_user, None).await.unwrap();

		// Fetch expected outcomes from the client directly for verification:
		let best_block_answer = client.get_best_block().await.unwrap().0;
		let block_answer = client.get_block(&best_block_answer).await.unwrap();

		let header_answer = match block_answer {
			BlockData::HeaderOnly(header) => header,
			BlockData::FullBlock(block) => block.header,
		};

		let best_header_answer = client.get_header(&best_block_answer, None).await.unwrap();

		assert_eq!(best_block_user, best_block_answer);

		assert_eq!(header_user, header_answer);

		assert_eq!(best_header_user, best_header_answer);
	}

	#[tokio::test]
	async fn test_05_broadcast_transactions() {
		let client = get_bitcoind_client().await;

		let tx = "0100000001d611ad58b2f5bc0db7d15dfde4f497d6482d1b4a1e8c462ef077d4d32b3dae7901000000da0047304402203b17b4f64fa7299e8a85a688bda3cb1394b80262598bbdffd71dab1d7f266098022019cc20dc20eae417374609cb9ca22b28261511150ed69d39664b9d3b1bcb3d1201483045022100cfff9c400abb4ce5f247bd1c582cf54ec841719b0d39550b714c3c793fb4347b02201427a961a7f32aba4eeb1b71b080ea8712705e77323b747c03c8f5dbdda1025a01475221032d7306898e980c66aefdfb6b377eaf71597c449bf9ce741a3380c5646354f6de2103e8c742e1f283ef810c1cd0c8875e5c2998a05fc5b23c30160d3d33add7af565752aeffffffff020ed000000000000016001477800cff52bd58133b895622fd1220d9e2b47a79cd0902000000000017a914da55145ca5c56ba01f1b0b98d896425aa4b0f4468700000000";

		let transaction = encode::deserialize(&hex_utils::to_vec(&tx).unwrap()).unwrap();

		client.broadcast_transactions(&[&transaction]);

		tokio::time::sleep(Duration::from_millis(250)).await;
	}

	#[tokio::test]
	async fn test_06_get_est_sat_per_1000_weight() {
		let client = get_bitcoind_client().await;

		//println!("fees: {:?}", client.fees);

		let feerate_target = ConfirmationTarget::MaximumFeeEstimate;

		let fees = client.get_est_sat_per_1000_weight(feerate_target);

		assert_eq!(fees, 50000);
	}

	#[tokio::test]
	async fn test_07_create_funding_transaction() {
		let wallet = get_wallet().await;

		let script = ScriptBuf::new();
		let sats: u64 = 1000;

		let confirmation_target = ConfirmationTarget::AnchorChannelFee;

		let locktime = LockTime::ZERO;

		let channel_amount = Amount::from_sat(sats);

		let tx: Transaction = wallet.create_funding_transaction(
			script,
			channel_amount,
			confirmation_target,
			locktime,
		);

		let tx_id = "8195f33e75091ff63814c6cba4b47bc0c66e2a1aa47ce4c88e4fbf5219165d28"
			.parse::<Txid>()
			.unwrap();

		assert_eq!(tx.vsize(), 119,);
	}

	#[test]
	fn test_08_filesystemstore_right() -> io::Result<()> {
		let mut temp_path = temp_dir();
		temp_path.push("simple_store_test");
		let store = ExerciseFileStore::new(temp_path);

		// Test write and read
		store.write("test", "user1", "key1", b"hello world")?;
		let data = store.read("test", "user1", "key1")?;
		assert_eq!(data, b"hello world");

		// Test list
		let keys = store.list("test", "user1")?;
		assert_eq!(keys, vec!["key1"]);

		// Test remove
		store.remove("test", "user1", "key1", false)?;
		let keys = store.list("test", "user1")?;
		assert_eq!(keys.len(), 0);

		// Test NotFound error
		match store.read("test", "user1", "nonexistent") {
			Err(e) if e.kind() == ErrorKind::NotFound => (),
			_ => panic!("Expected NotFound error"),
		}

		Ok(())
	}

	#[tokio::test]
	async fn test_09_start_network_listener() {
		// Setup
		let listening_port = 9735; // Fixed port instead of 0
		let peer_manager = Arc::new(MockPeerManager::new());

		// Start the listener
		start_network_listener(peer_manager.clone(), listening_port).await;

		// Wait briefly for the listener to bind
		tokio::time::sleep(Duration::from_millis(100)).await;

		// Discover the assigned port
		let temp_listener = tokio::net::TcpListener::bind("[::]:0")
			.await
			.expect("Failed to bind temporary listener");
		let assigned_port = temp_listener.local_addr().unwrap().port();
		drop(temp_listener);

		// Test 1: Connect to the listener
		let client_stream = timeout(
			Duration::from_secs(1),
			TcpStream::connect(format!("127.0.0.1:{}", listening_port)),
		)
		.await
		.expect("Connection timed out")
		.expect("Failed to connect to listener");
		println!("Successfully connected to listener on port {}", listening_port);

		// Wait for setup_inbound to process
		tokio::time::sleep(Duration::from_millis(100)).await;
	}

	#[tokio::test]
	async fn test_10_handle_ldk_events() {
		let channel_manager = ChannelManager::new();
		let bitcoin_client = BitcoinClient::new();
		let keys_manager = KeysManager::new();
		let peer_manager = PeerManager::new();
		let file_store = FileStore::new();
		//let onchain_wallet = MockOnChainWallet::new();
		let onchain_wallet = get_wallet().await;
		let wallet = &*onchain_wallet;

		let temporary_channel_id = ChannelId::new_zero();
		let user_channel_id = 0;
		let counterparty_node_id = pubkey_from_private_key(&[0x01; 32]);
		let channel_value_satoshis = 1_000_000;
		let output_script = Builder::new().push_int(16).push_slice(&[0, 40]).into_script();

		let event = Event::FundingGenerationReady {
			temporary_channel_id,
			counterparty_node_id,
			channel_value_satoshis,
			output_script,
			user_channel_id,
		};

		handle_ldk_events(
			&channel_manager,
			bitcoin_client,
			wallet,
			keys_manager,
			peer_manager,
			file_store,
			event,
		)
		.await;

		// Check final state
		let final_funding_tx = channel_manager.funding_tx.lock().unwrap();
		

		assert!(final_funding_tx.is_some(), "Funding transaction should be present");
	}

	#[tokio::test]
	async fn test_11_open_channel() {
		let channel_manager = ChannelManager::new();
		let peer_pubkey = pubkey_from_private_key(&[0x01; 32]);
		let channel_amt_sat = 1_000_000;
		let announce_for_forwarding = false;
		let with_anchors = true;

		open_channel(
			peer_pubkey,
			channel_amt_sat,
			announce_for_forwarding,
			with_anchors,
			&channel_manager,
		);

		// Check final state
		let final_channel = channel_manager.channel.lock().unwrap();

		assert!(final_channel.is_some(), "New channel should be present");

		// Check UserConfig
		if let Some((user_channel_id, channel_value_satoshis, config)) = &*final_channel {
				assert_eq!(config.channel_handshake_config.their_channel_reserve_proportional_millionths, 1_000);
				assert_eq!(config.channel_handshake_config.negotiate_anchors_zero_fee_htlc_tx, true);
		} else {
				panic!("Channel configuration incorrect");
		}
	}

	/*
	#[test]
	fn test_13_outbound_payment() {
		// Step 1: Setup empty storage
		let mut outbound_payments = OutboundPaymentInfoStorage { payments: HashMap::new() };

		// Step 2: Add a payment
		let payment_id = Some(PaymentId([0x01; 32]));
		let payment_secret = Some(PaymentSecret([0x01; 32]));
		let amount = SatAmount(Some(1000));
		let payment_info = PaymentInfo {
			preimage: None,
			secret: payment_secret,
			status: HTLCStatus::Pending,
			amt_msat: amount,
		};
		outbound_payments.payments.insert(payment_id.expect("Valid ID"), payment_info);

		// Step 3: Verify it was added correctly
		assert_eq!(outbound_payments.payments.len(), 1, "Should have one payment");
	}
	*/

	#[test]
	fn test_12_send_payment() {
		let channel_manager = ChannelManager::new();
		let mut outbound_payments = OutboundPaymentInfoStorage { payments: HashMap::new() };
		let required_amount_msat = Some(250_000_000);
		let file_store = FileStore::new();

		let invoice = Bolt11Invoice::from_str( "lnbc2500u1pvjluezsp5zyg3zyg3zyg3zyg3zyg3zyg3zyg3zyg3zyg3zyg3zyg3zyg3zygspp5qqqsyqcyq5rqwzqfqqqsyqcyq5rqwzqfqqqsyqcyq5rqwzqfqypqdq5xysxxatsyp3k7enxv4jsxqzpu9qrsgquk0rl77nj30yxdy8j9vdx85fkpmdla2087ne0xh8nhedh8w27kyke0lp53ut353s06fv3qfegext0eh0ymjpf39tuven09sam30g4vgpfna3rh").expect("Valid invoice");

		send_payment(
			&channel_manager,
			&invoice,
			required_amount_msat,
			&mut outbound_payments,
			file_store,
		);

		// Check final state
		let final_payments = channel_manager.payment.lock().unwrap();
		println!("Final channels: {:?}", final_payments);

		assert!(final_payments.is_some(), "New payment should be present");
	}

}

fn pubkey_from_private_key(private_key: &[u8; 32]) -> PublicKey {
	let secp = Secp256k1::new();
	let secret_key = secp256k1::SecretKey::from_slice(private_key).unwrap();
	secp256k1::PublicKey::from_secret_key(&secp, &secret_key)
}
