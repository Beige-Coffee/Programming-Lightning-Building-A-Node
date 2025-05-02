#![allow(dead_code, unused_imports, unused_variables,unused_mut, unused_must_use, unexpected_cfgs, elided_named_lifetimes)]
use crate::convert::{
	BlockchainInfo, FeeResponse, FundedTx, ListUnspentResponse, MempoolMinFeeResponse, NewAddress,
	RawTx, SignedTx, MempoolInfo
};
use crate::logger::FilesystemLogger;
use crate::hex_utils;
use base64;
use bitcoin::address::Address;
use bitcoin::blockdata::constants::WITNESS_SCALE_FACTOR;
use bitcoin::blockdata::script::ScriptBuf;
use bitcoin::blockdata::transaction::Transaction;
use bitcoin::consensus::{encode, Decodable, Encodable};
use bitcoin::hash_types::{BlockHash, Txid};
use bitcoin::hashes::Hash;
use bitcoin::key::XOnlyPublicKey;
use bitcoin::psbt::Psbt;
use bitcoin::{Network, OutPoint, TxOut, WPubkeyHash};
use lightning::chain::chaininterface::{BroadcasterInterface, ConfirmationTarget, FeeEstimator};
use lightning::events::bump_transaction::{Utxo, WalletSource};
use lightning::log_error;
use lightning::sign::ChangeDestinationSource;
use lightning::util::logger::Logger;
use lightning_block_sync::http::HttpEndpoint;
use lightning_block_sync::rpc::RpcClient;
use lightning_block_sync::{AsyncBlockSourceResult, BlockData, BlockHeaderData, BlockSource};
use lightning::log_info;
use serde_json;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// The minimum feerate we are allowed to send, as specify by LDK.
const MIN_FEERATE: u32 = 253;

////////////////////////////
// START Exercise 1 //
// Implement `new`
////////////////////////////

pub struct BitcoindClient {
	pub(crate) bitcoind_rpc_client: Arc<RpcClient>,
	network: Network,
	host: String,
	port: u16,
	rpc_user: String,
	rpc_password: String,
	pub fees: Arc<HashMap<ConfirmationTarget, AtomicU32>>,
	handle: tokio::runtime::Handle,
	logger: Arc<FilesystemLogger>,
}

impl BitcoindClient {
	pub(crate) async fn new(
		host: String, port: u16, rpc_user: String, rpc_password: String, network: Network,
		handle: tokio::runtime::Handle, logger: Arc<FilesystemLogger>,
	) -> std::io::Result<Self> {
		let http_endpoint = HttpEndpoint::for_host(host.clone()).with_port(port);
		let rpc_credentials =
			base64::encode(format!("{}:{}", rpc_user.clone(), rpc_password.clone()));
		let bitcoind_rpc_client = RpcClient::new(&rpc_credentials, http_endpoint)?;

		let mut fees: HashMap<ConfirmationTarget, AtomicU32> = HashMap::new();

		fees.insert(ConfirmationTarget::MaximumFeeEstimate, AtomicU32::new(50000));
		fees.insert(ConfirmationTarget::UrgentOnChainSweep, AtomicU32::new(5000));
		fees.insert(
				ConfirmationTarget::MinAllowedAnchorChannelRemoteFee,
				AtomicU32::new(MIN_FEERATE),
		);
		fees.insert(
				ConfirmationTarget::MinAllowedNonAnchorChannelRemoteFee,
				AtomicU32::new(MIN_FEERATE),
		);
		fees.insert(ConfirmationTarget::AnchorChannelFee, AtomicU32::new(MIN_FEERATE));
		fees.insert(ConfirmationTarget::NonAnchorChannelFee, AtomicU32::new(2000));
		fees.insert(ConfirmationTarget::ChannelCloseMinimum, AtomicU32::new(MIN_FEERATE));
		fees.insert(ConfirmationTarget::OutputSpendingFee, AtomicU32::new(MIN_FEERATE));


		let client = Self {
			bitcoind_rpc_client: Arc::new(bitcoind_rpc_client),
			host,
			port,
			rpc_user,
			rpc_password,
			network,
			fees: Arc::new(fees),
			handle: handle.clone(),
			logger,
		};

		BitcoindClient::poll_for_fee_estimates(
				client.fees.clone(),
				client.bitcoind_rpc_client.clone(),
				handle,
		);
		
		Ok(client)
	}

	////////////////////////////
	// END Exercise 1 //
	////////////////////////////

}

////////////////////////////
// START Exercise 2 //
////////////////////////////

impl BlockSource for BitcoindClient {
	fn get_header<'a>(
		&'a self, header_hash: &'a BlockHash, height_hint: Option<u32>,
	) -> AsyncBlockSourceResult<'a, BlockHeaderData> {
		Box::pin(async move { 
			let header_hash = serde_json::json!(header_hash.to_string());
			Ok(self.bitcoind_rpc_client.call_method("getblockheader", &[header_hash]).await?)
		})
	}

	fn get_block<'a>(
		&'a self, header_hash: &'a BlockHash,
	) -> AsyncBlockSourceResult<'a, BlockData> {
		Box::pin( async move {
			let header_hash = serde_json::json!(header_hash.to_string());
			let verbosity = serde_json::json!(0);
			Ok(BlockData::FullBlock(self.bitcoind_rpc_client.call_method("getblock", &[header_hash, verbosity]).await?))
		})
	}

	fn get_best_block<'a>(&'a self) -> AsyncBlockSourceResult<(BlockHash, Option<u32>)> {
		Box::pin(async move {
			Ok(self.bitcoind_rpc_client.call_method("getblockchaininfo", &[]).await?)
		})
	}
}

////////////////////////////
// END Exercise 2 //
////////////////////////////


////////////////////////////
// START Exercise 3 //
////////////////////////////


impl BroadcasterInterface for BitcoindClient {
	fn broadcast_transactions(&self, txs: &[&Transaction]) {
		let txn = txs.iter().map(|tx| encode::serialize_hex(tx)).collect::<Vec<_>>();
		for tx in txn {
			let bitcoind_rpc_client = Arc::clone(&self.bitcoind_rpc_client);
			let logger = Arc::clone(&self.logger);
			self.handle.spawn(async move {
				let tx_json = serde_json::json!(tx);
				match bitcoind_rpc_client
					.call_method::<serde_json::Value>("sendrawtransaction", &[tx_json])
					.await

				{
					Ok(result) => {
						log_info!(logger, "Successfully broadcasted transaction: {:?}", result);
					},
					Err(e) => {
						log_error!(logger, "Failed to broadcast transaction: {:?}", e);
					}
				}
			});
		}
	}
}

////////////////////////////
// END Exercise 3 //
////////////////////////////


////////////////////////////
// START Exercise 4 //
////////////////////////////

impl BitcoindClient {
	fn poll_for_fee_estimates(
		fees: Arc<HashMap<ConfirmationTarget, AtomicU32>>, rpc_client: Arc<RpcClient>,
		handle: tokio::runtime::Handle,
	) {
		handle.spawn(async move {
			loop {
				let mempoolmin_estimate = {
					let resp = rpc_client
						.call_method::<MempoolMinFeeResponse>("getmempoolinfo", &vec![])
						.await
						.unwrap();
					match resp.feerate_sat_per_kw {
						Some(feerate) => std::cmp::max(feerate, MIN_FEERATE),
						None => MIN_FEERATE,
					}
				};
				let background_estimate = {
					let background_conf_target = serde_json::json!(144);
					let background_estimate_mode = serde_json::json!("ECONOMICAL");
					let resp = rpc_client
						.call_method::<FeeResponse>(
							"estimatesmartfee",
							&vec![background_conf_target, background_estimate_mode],
						)
						.await
						.unwrap();
					match resp.feerate_sat_per_kw {
						Some(feerate) => std::cmp::max(feerate, MIN_FEERATE),
						None => MIN_FEERATE,
					}
				};

				let normal_estimate = {
					let normal_conf_target = serde_json::json!(18);
					let normal_estimate_mode = serde_json::json!("ECONOMICAL");
					let resp = rpc_client
						.call_method::<FeeResponse>(
							"estimatesmartfee",
							&vec![normal_conf_target, normal_estimate_mode],
						)
						.await
						.unwrap();
					match resp.feerate_sat_per_kw {
						Some(feerate) => std::cmp::max(feerate, MIN_FEERATE),
						None => 2000,
					}
				};

				let high_prio_estimate = {
					let high_prio_conf_target = serde_json::json!(6);
					let high_prio_estimate_mode = serde_json::json!("CONSERVATIVE");
					let resp = rpc_client
						.call_method::<FeeResponse>(
							"estimatesmartfee",
							&vec![high_prio_conf_target, high_prio_estimate_mode],
						)
						.await
						.unwrap();

					match resp.feerate_sat_per_kw {
						Some(feerate) => std::cmp::max(feerate, MIN_FEERATE),
						None => 5000,
					}
				};

				let very_high_prio_estimate = {
					let high_prio_conf_target = serde_json::json!(2);
					let high_prio_estimate_mode = serde_json::json!("CONSERVATIVE");
					let resp = rpc_client
						.call_method::<FeeResponse>(
							"estimatesmartfee",
							&vec![high_prio_conf_target, high_prio_estimate_mode],
						)
						.await
						.unwrap();

					match resp.feerate_sat_per_kw {
						Some(feerate) => std::cmp::max(feerate, MIN_FEERATE),
						None => 50000,
					}
				};

				fees.get(&ConfirmationTarget::MaximumFeeEstimate)
					.unwrap()
					.store(very_high_prio_estimate, Ordering::Release);
				fees.get(&ConfirmationTarget::UrgentOnChainSweep)
					.unwrap()
					.store(high_prio_estimate, Ordering::Release);
				fees.get(&ConfirmationTarget::MinAllowedAnchorChannelRemoteFee)
					.unwrap()
					.store(mempoolmin_estimate, Ordering::Release);
				fees.get(&ConfirmationTarget::MinAllowedNonAnchorChannelRemoteFee)
					.unwrap()
					.store(background_estimate - 250, Ordering::Release);
				fees.get(&ConfirmationTarget::AnchorChannelFee)
					.unwrap()
					.store(background_estimate, Ordering::Release);
				fees.get(&ConfirmationTarget::NonAnchorChannelFee)
					.unwrap()
					.store(normal_estimate, Ordering::Release);
				fees.get(&ConfirmationTarget::ChannelCloseMinimum)
					.unwrap()
					.store(background_estimate, Ordering::Release);
				fees.get(&ConfirmationTarget::OutputSpendingFee)
					.unwrap()
					.store(background_estimate, Ordering::Release);

				tokio::time::sleep(Duration::from_secs(60)).await;
			}
		});
	}
}

impl FeeEstimator for BitcoindClient {
	fn get_est_sat_per_1000_weight(&self, confirmation_target: ConfirmationTarget) -> u32 {
		self.fees.get(&confirmation_target).unwrap().load(Ordering::Acquire)
	}
}


	////////////////////////////
	// END Exercise 4 //
	////////////////////////////

impl BitcoindClient {

	pub fn get_new_rpc_client(&self) -> std::io::Result<RpcClient> {
		let http_endpoint = HttpEndpoint::for_host(self.host.clone()).with_port(self.port);
		let rpc_credentials =
			base64::encode(format!("{}:{}", self.rpc_user.clone(), self.rpc_password.clone()));
		RpcClient::new(&rpc_credentials, http_endpoint)
	}

	pub async fn create_raw_transaction(&self, outputs: Vec<HashMap<String, f64>>) -> RawTx {
		let outputs_json = serde_json::json!(outputs);
		self.bitcoind_rpc_client
			.call_method::<RawTx>(
				"createrawtransaction",
				&vec![serde_json::json!([]), outputs_json],
			)
			.await
			.unwrap()
	}

	pub async fn fund_raw_transaction(&self, raw_tx: RawTx) -> FundedTx {
		let raw_tx_json = serde_json::json!(raw_tx.0);
		let options = serde_json::json!({
			// LDK gives us feerates in satoshis per KW but Bitcoin Core here expects fees
			// denominated in satoshis per vB. First we need to multiply by 4 to convert weight
			// units to virtual bytes, then divide by 1000 to convert KvB to vB.
			"fee_rate": self
				.get_est_sat_per_1000_weight(ConfirmationTarget::NonAnchorChannelFee) as f64 / 250.0,
			// While users could "cancel" a channel open by RBF-bumping and paying back to
			// themselves, we don't allow it here as its easy to have users accidentally RBF bump
			// and pay to the channel funding address, which results in loss of funds. Real
			// LDK-based applications should enable RBF bumping and RBF bump either to a local
			// change address or to a new channel output negotiated with the same node.
			"replaceable": false,
		});
		self.bitcoind_rpc_client
			.call_method("fundrawtransaction", &[raw_tx_json, options])
			.await
			.unwrap()
	}

	pub async fn send_raw_transaction(&self, raw_tx: RawTx) {
		let raw_tx_json = serde_json::json!(raw_tx.0);
		self.bitcoind_rpc_client
			.call_method::<Txid>("sendrawtransaction", &[raw_tx_json])
			.await
			.unwrap();
	}

	pub async fn sign_raw_transaction_with_wallet(&self, tx_hex: String) -> SignedTx {
		let tx_hex_json = serde_json::json!(tx_hex);
		self.bitcoind_rpc_client
			.call_method("signrawtransactionwithwallet", &vec![tx_hex_json])
			.await
			.unwrap()
	}

	pub async fn get_new_address(&self) -> Address {
		let addr_args = vec![serde_json::json!("LDK output address")];
		let addr = self
			.bitcoind_rpc_client
			.call_method::<NewAddress>("getnewaddress", &addr_args)
			.await
			.unwrap();
		Address::from_str(addr.0.as_str()).unwrap().require_network(self.network).unwrap()
	}

	pub async fn get_blockchain_info(&self) -> BlockchainInfo {
		self.bitcoind_rpc_client
			.call_method::<BlockchainInfo>("getblockchaininfo", &vec![])
			.await
			.unwrap()
	}

	pub async fn get_raw_mempool(&self) -> MempoolInfo {
	self.bitcoind_rpc_client
	.call_method("getrawmempool", &[])
	.await
	.unwrap()
	}

	pub async fn list_unspent(&self) -> ListUnspentResponse {
		self.bitcoind_rpc_client
			.call_method::<ListUnspentResponse>("listunspent", &vec![])
			.await
			.unwrap()
	}
}

impl ChangeDestinationSource for BitcoindClient {
	fn get_change_destination_script(&self) -> Result<ScriptBuf, ()> {
		tokio::task::block_in_place(move || {
			Ok(self.handle.block_on(async move { self.get_new_address().await.script_pubkey() }))
		})
	}
}

impl WalletSource for BitcoindClient {
	fn list_confirmed_utxos(&self) -> Result<Vec<Utxo>, ()> {
		let utxos = tokio::task::block_in_place(move || {
			self.handle.block_on(async move { self.list_unspent().await }).0
		});
		Ok(utxos
			.into_iter()
			.filter_map(|utxo| {
				let outpoint = OutPoint { txid: utxo.txid, vout: utxo.vout };
				let value = bitcoin::Amount::from_sat(utxo.amount);
				match utxo.address.witness_program() {
					Some(prog) if prog.is_p2wpkh() => {
						WPubkeyHash::from_slice(prog.program().as_bytes())
							.map(|wpkh| Utxo::new_v0_p2wpkh(outpoint, value, &wpkh))
							.ok()
					},
					Some(prog) if prog.is_p2tr() => {
						// TODO: Add `Utxo::new_v1_p2tr` upstream.
						XOnlyPublicKey::from_slice(prog.program().as_bytes())
							.map(|_| Utxo {
								outpoint,
								output: TxOut {
									value,
									script_pubkey: utxo.address.script_pubkey(),
								},
								satisfaction_weight: 1 /* empty script_sig */ * WITNESS_SCALE_FACTOR as u64 +
									1 /* witness items */ + 1 /* schnorr sig len */ + 64, /* schnorr sig */
							})
							.ok()
					},
					_ => None,
				}
			})
			.collect())
	}

	fn get_change_script(&self) -> Result<ScriptBuf, ()> {
		tokio::task::block_in_place(move || {
			Ok(self.handle.block_on(async move { self.get_new_address().await.script_pubkey() }))
		})
	}

	fn sign_psbt(&self, tx: Psbt) -> Result<Transaction, ()> {
		let mut tx_bytes = Vec::new();
		let _ = tx.unsigned_tx.consensus_encode(&mut tx_bytes).map_err(|_| ());
		let tx_hex = hex_utils::hex_str(&tx_bytes);
		let signed_tx = tokio::task::block_in_place(move || {
			self.handle.block_on(async move { self.sign_raw_transaction_with_wallet(tx_hex).await })
		});
		let signed_tx_bytes = hex_utils::to_vec(&signed_tx.hex).ok_or(())?;
		Transaction::consensus_decode(&mut signed_tx_bytes.as_slice()).map_err(|_| ())
	}
}
