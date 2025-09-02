//use bdk_wallet::Wallet as BdkWallet;
use bdk_esplora::esplora_client::{Builder, BlockingClient};
use bdk_esplora::{esplora_client, EsploraExt};
use bdk_chain::ChainPosition::{Confirmed, Unconfirmed};
use bdk_wallet::rusqlite::Connection;
use bdk_wallet::PersistedWallet;
use bdk_wallet::{
	bitcoin::{Block, Network},
	KeychainKind, SignOptions, Wallet,
};
use ::bdk_wallet::Wallet as BdkWallet;
use bitcoin::network::Network as BitcoinNetwork;
use std::{collections::BTreeSet, io::Write};
use bdk_wallet::{AddressInfo, Balance};
use bitcoin::address::Address;
use bitcoin::blockdata::constants::WITNESS_SCALE_FACTOR;
use bitcoin::blockdata::script::ScriptBuf;
use bitcoin::blockdata::transaction::Transaction;
use bitcoin::hashes::Hash;
use bitcoin::key::XOnlyPublicKey;
use bitcoin::psbt::Psbt;
use bitcoin::FeeRate;
use bitcoin::{OutPoint, TxOut, WPubkeyHash};
use lightning::chain::chaininterface::{BroadcasterInterface, ConfirmationTarget, FeeEstimator};
use lightning::events::bump_transaction::{Utxo, WalletSource};
use lightning::log_info;
use lightning::sign::ChangeDestinationSource;
use lightning::util::logger::Logger;
use std::collections::HashMap;
use std::ops::Deref;
use std::sync::{Mutex, RwLock};
use bitcoin::blockdata::locktime::absolute::LockTime;
use bitcoin::{
	Amount,
};
use std::{
	path::PathBuf,
	sync::{mpsc::sync_channel, Arc},
	thread::spawn,
	time::Instant,
};
use lightning::log_error;
use ::bdk_wallet::template::Bip84;


const STOP_GAP: usize = 5;
const PARALLEL_REQUESTS: usize = 5;


/// On-chain Bitcoin wallet with BDK
/// 
/// This struct wraps a BDK wallet and provides the functionality used in Lightning
/// operations like funding channels, sweeping outputs, and managing on-chain funds.
/// It implements LDK traits (ChangeDestinationSource, WalletSource) to integrate with an LDK Lightning node.
///
/// Generic parameters:
/// - B: Broadcaster for sending transactions to the Bitcoin network
/// - E: Fee estimator for calculating appropriate transaction fees  
/// - L: Logger for recording wallet operations

pub(crate) struct OnChainWallet<B: Deref, E: Deref, L: Deref>
where
	B::Target: BroadcasterInterface,
	E::Target: FeeEstimator,
	L::Target: Logger,
{
	/// The underlying BDK wallet, protected by a mutex for thread safety
	inner: Mutex<PersistedWallet<Connection>>,
	/// Esplora client for blockchain data (blocks, transactions, UTXOs)
	client: Arc<BlockingClient>,
	/// Path to the SQLite database file storing wallet data
	path_to_db: String,
	/// Transaction broadcaster (BitcoindClient)
	broadcaster: B,
	/// Fee estimator (BitcoindClient)
	fee_estimator: E,
	/// Logger for recording operations and errors
	logger: L,
}

impl<B: Deref, E: Deref, L: Deref> OnChainWallet<B, E, L>
where
	B::Target: BroadcasterInterface,
	E::Target: FeeEstimator,
	L::Target: Logger,
{
	/// Creates a new on-chain wallet from a seed phrase
	/// 
	/// This method encapsulates all the BDK wallet construction logic,
	/// including descriptor creation, database setup, and blockchain client initialization.
	/// 
	/// # Arguments
	/// * `keys_seed` - 32-byte seed for deriving wallet keys
	/// * `network` - Bitcoin network (mainnet, testnet, regtest, signet)
	/// * `path_to_db` - Path to SQLite database file for wallet persistence
	/// * `fee_estimator` - Component for estimating transaction fees
	/// * `broadcaster` - Component for broadcasting transactions
	/// * `logger` - Component for logging operations
	/// 
	/// # Returns
	/// Configured OnChainWallet ready for Lightning operations
	pub(crate) fn new_from_seed(
		keys_seed: &[u8], network: BitcoinNetwork,
		path_to_db: &str, fee_estimator: E, broadcaster: B, logger: L,
	) -> Self {

		// Derive the master extended private key from the seed
		let xprv = bitcoin::bip32::Xpriv::new_master(network, &keys_seed).unwrap();
		// External keychain: for receiving payments and change outputs
		let descriptor = Bip84(xprv, KeychainKind::External);
		// Internal keychain: for change addresses (not shown to users)
		let change_descriptor = Bip84(xprv, KeychainKind::Internal);

		// Open SQLite database connection for wallet persistence
		let mut conn = ::bdk_wallet::rusqlite::Connection::open(path_to_db).unwrap();

		// Try to load existing wallet from database
		let wallet_opt = BdkWallet::load()
			.descriptor(KeychainKind::External, Some(descriptor.clone()))
			.descriptor(KeychainKind::Internal, Some(change_descriptor.clone()))
			.extract_keys()
			.check_network(network)
			.load_wallet(&mut conn)
			.map_err(|e| {
				log_error!(logger, "Failed to load BDK wallet: {:?}", e);
			})
			.unwrap();

		// Create new wallet if none exists, otherwise use loaded wallet
		let bdk_wallet = match wallet_opt {
			Some(wallet) => wallet,
			None => BdkWallet::create(descriptor, change_descriptor)
				.network(network)
				.create_wallet(&mut conn)
				.map_err(|e| {
					log_error!(logger, "Failed to set up wallet: {}", e);
				})
				.unwrap(),
		};

		// Wrap BDK wallet in mutex for thread-safe access
		let inner = Mutex::new(bdk_wallet);

		// Initialize Esplora client for blockchain data
		let esplora_url = "https://01c81926ec00.ngrok.app".to_string();
		let client = Arc::new(esplora_client::Builder::new(&esplora_url).build_blocking());

		// Create the wallet instance
		let this = Self { inner, client, path_to_db: path_to_db.to_string(), broadcaster, fee_estimator, logger };

		// Perform initial blockchain scan to discover existing transactions
		this.full_scan();

		this
	}

	/// Performs a full blockchain scan to discover wallet transactions
	/// 
	/// This scans the entire blockchain history for transactions belonging to this wallet.
	/// Should only be called during initial setup or wallet recovery.
	pub fn full_scan(&self) -> anyhow::Result<(), Box<dyn std::error::Error>> {
		let mut wallet = self.inner.lock().unwrap(); 
		let mut db = Connection::open(self.path_to_db.clone()).unwrap();
		
		let request = wallet.start_full_scan().inspect({
			let mut stdout = std::io::stdout();
			let mut once = BTreeSet::<KeychainKind>::new();
			move |keychain, spk_i, _| {
					if once.insert(keychain) {
							//print!("\nScanning keychain [{keychain:?}] ");
					}
					if spk_i % 5 == 0 {
							//print!(" {spk_i:<3}");
					}
					stdout.flush().expect("must flush")
			}
		});

		let update = self.client.full_scan(request, STOP_GAP, PARALLEL_REQUESTS)?;
		wallet.apply_update(update)?;
		wallet.persist(&mut db)?;
		Ok(())
		
	}

	/// Synchronizes wallet with latest blockchain state
	/// 
	/// This is more efficient than full_scan as it only checks for new transactions
	/// since the last sync. Should be called regularly to keep wallet up-to-date.
	pub fn sync_wallet(&self) -> anyhow::Result<(), Box<dyn std::error::Error>> {
		let mut wallet = self.inner.lock().unwrap();
		let mut db = Connection::open(self.path_to_db.clone()).unwrap();

		let mut printed = 0;
		let sync_request = wallet
				.start_sync_with_revealed_spks()
				.inspect(move |_, sync_progress| {
						let progress_percent =
								(100 * sync_progress.consumed()) as f32 / sync_progress.total() as f32;
						let progress_percent = progress_percent.round() as u32;
						if progress_percent % 5 == 0 && progress_percent > printed {
								std::io::stdout().flush().expect("must flush");
								printed = progress_percent;
						}
				});
		let sync_update = self.client.sync(sync_request, PARALLEL_REQUESTS)?;

		wallet.apply_update(sync_update)?;
		wallet.persist(&mut db)?;

		Ok(())
	}

	/// Creates a Bitcoin transaction with the specified outputs
	/// 
	/// This method handles the complete transaction creation process:
	/// selecting UTXOs, calculating fees, and signing the transaction.
	pub fn create_funding_transaction(&self,
									output_script: ScriptBuf,
									amount: Amount,
									confirmation_target: ConfirmationTarget,
									locktime: LockTime) -> Transaction {
		// get lock on wallet
		let mut wallet = self.inner.lock().unwrap();

		// create tx builder
		let mut tx_builder = wallet.build_tx();

		// get fee rate, assuming normal fees
		let fee_rate =
			self.fee_estimator.get_est_sat_per_1000_weight(confirmation_target) as u64;
		let fees = FeeRate::from_sat_per_kwu(fee_rate);

		tx_builder.add_recipient(output_script, amount).fee_rate(fees).nlocktime(locktime);

		// build the transaction
		let mut psbt = tx_builder.finish().unwrap();

		let sign_options = SignOptions::default();

		wallet.sign(&mut psbt, sign_options).unwrap();

		let tx: Transaction = psbt.extract_tx().unwrap();

		tx
	}

	/// Generates a new Bitcoin address for receiving funds
	/// 
	/// Uses the external keychain to generate fresh receive addresses.
	pub fn get_address(&self) -> Address {
		let mut locked_wallet = self.inner.lock().unwrap();

		let address_info = locked_wallet.reveal_next_address(KeychainKind::External);

		address_info.address
	}

	/// Gets the current wallet balance
	pub fn get_balance(&self) -> Balance {
		let locked_wallet = self.inner.lock().unwrap();

		let balance = locked_wallet.balance();

		balance
	}
}

/// Implementation of LDK's WalletSource trait
impl<B, E, L> WalletSource for OnChainWallet<B, E, L>
where
	B: Deref<Target: BroadcasterInterface>,
	E: Deref<Target: FeeEstimator>,
	L: Deref<Target: Logger>,
{
	fn list_confirmed_utxos(&self) -> Result<Vec<Utxo>, ()> {
		let wallet = self.inner.lock().unwrap();

		let utxos: Vec<Utxo> = wallet
			.list_unspent()
			.filter(|utxo| utxo.chain_position.is_confirmed())
			.filter_map(|utxo| {
				let outpoint = OutPoint { txid: utxo.outpoint.txid, vout: utxo.outpoint.vout };
				let value = bitcoin::Amount::from_sat(utxo.txout.value.to_sat());
				let address =
					Address::from_script(&utxo.txout.script_pubkey, Network::Regtest).ok()?;

				match address.witness_program() {
					Some(prog) if prog.is_p2wpkh() => {
						WPubkeyHash::from_slice(prog.program().as_bytes())
							.map(|wpkh| Utxo::new_v0_p2wpkh(outpoint, value, &wpkh))
							.ok()
					},
					Some(prog) if prog.is_p2tr() => {
						XOnlyPublicKey::from_slice(prog.program().as_bytes())
							.map(|_| Utxo {
								outpoint,
								output: TxOut { value, script_pubkey: utxo.txout.script_pubkey },
								satisfaction_weight: 1 * WITNESS_SCALE_FACTOR as u64 + // empty script_sig
                                1 + // witness items
                                1 + // schnorr sig len
                                64, // schnorr sig
							})
							.ok()
					},
					_ => None,
				}
			})
			.collect();

		Ok(utxos)
	}

	fn get_change_script(&self) -> Result<ScriptBuf, ()> {
		let mut locked_wallet = self.inner.lock().unwrap();

		let address_info = locked_wallet.reveal_next_address(KeychainKind::External);

		Ok(address_info.address.script_pubkey())
	}

	fn sign_psbt(&self, tx: Psbt) -> Result<Transaction, ()> {
		let wallet = self.inner.lock().unwrap();

		let sign_options = SignOptions { trust_witness_utxo: true, ..Default::default() };

		let mut psbt = tx;

		wallet.sign(&mut psbt, sign_options).unwrap();

		let signed_tx: Transaction = psbt.extract_tx().unwrap();

		Ok(signed_tx)
	}
}

// Implementation of LDK's ChangeDestinationSource trait
impl<B, E, L> ChangeDestinationSource for OnChainWallet<B, E, L>
where
	B: Deref<Target: BroadcasterInterface>,
	E: Deref<Target: FeeEstimator>,
	L: Deref<Target: Logger>,
{
	fn get_change_destination_script(&self) -> Result<ScriptBuf, ()> {
		let mut locked_wallet = self.inner.lock().unwrap();

		let address_info = locked_wallet.reveal_next_address(KeychainKind::External);

		Ok(address_info.address.script_pubkey())
	}
}
