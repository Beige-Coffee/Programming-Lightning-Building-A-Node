//use bdk_wallet::Wallet as BdkWallet;
use bdk_esplora::esplora_client::{Builder, BlockingClient};
use bdk_esplora::{esplora_client, EsploraExt};
use bdk_chain::ChainPosition::{Confirmed, Unconfirmed};
use bdk_wallet::rusqlite::Connection;
use bdk_wallet::PersistedWallet as BdkWallet;
use bdk_wallet::{
	bitcoin::{Block, Network},
	KeychainKind, SignOptions, Wallet,
};
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
use std::{
	path::PathBuf,
	sync::{mpsc::sync_channel, Arc},
	thread::spawn,
	time::Instant,
};

const STOP_GAP: usize = 5;
const PARALLEL_REQUESTS: usize = 5;


pub(crate) struct OnChainWallet<B: Deref, E: Deref, L: Deref>
where
	B::Target: BroadcasterInterface,
	E::Target: FeeEstimator,
	L::Target: Logger,
{
	// A BDK on-chain wallet.
	inner: Mutex<BdkWallet<Connection>>,
	client: Arc<BlockingClient>,
	path_to_db: String,
	broadcaster: B,
	fee_estimator: E,
	logger: L,
}

impl<B: Deref, E: Deref, L: Deref> OnChainWallet<B, E, L>
where
	B::Target: BroadcasterInterface,
	E::Target: FeeEstimator,
	L::Target: Logger,
{
	pub(crate) fn new(
		wallet: BdkWallet<Connection>, host: String, port: u16, rpc_user: String,
		path_to_db: String, rpc_password: String, fee_estimator: E, broadcaster: B, logger: L,
	) -> Self {
		
		let inner = Mutex::new(wallet);

		let esplora_url = "https://ee5c65241ab6.ngrok.app".to_string();

		let client = Arc::new(esplora_client::Builder::new(&esplora_url).build_blocking());

		let this = Self { inner, client, path_to_db, broadcaster, fee_estimator, logger };

		this.full_scan();

		this
	}

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

	pub fn create_transaction(&self, outputs: Vec<HashMap<ScriptBuf, u64>>,
													 confirmation_target: ConfirmationTarget) -> Transaction {
		// get lock on wallet
		let mut wallet = self.inner.lock().unwrap();

		// create tx builder
		let mut tx_builder = wallet.build_tx();

		for output_map in outputs {
			for (script, sats) in output_map {
				let amount = bitcoin::Amount::from_sat(sats);
				tx_builder.add_recipient(script, amount);
			}
		}

		// get fee rate, assuming normal fees
		let fee_rate =
			self.fee_estimator.get_est_sat_per_1000_weight(confirmation_target);
		let fee_rate_vb = fee_rate as u64 / 4; // Convert sat/kw to sat/vB
		tx_builder.fee_rate(FeeRate::from_sat_per_vb(fee_rate_vb).unwrap());

		// build the transaction
		let mut psbt = tx_builder.finish().unwrap();

		let sign_options = SignOptions { trust_witness_utxo: true, ..Default::default() };

		wallet.sign(&mut psbt, sign_options).unwrap();

		let tx: Transaction = psbt.extract_tx().unwrap();

		tx
	}

	pub fn get_address(&self) -> Address {
		let mut locked_wallet = self.inner.lock().unwrap();

		let address_info = locked_wallet.reveal_next_address(KeychainKind::External);

		address_info.address
	}

	pub fn get_balance(&self) -> Balance {
		let locked_wallet = self.inner.lock().unwrap();

		let balance = locked_wallet.balance();

		balance
	}
}

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
