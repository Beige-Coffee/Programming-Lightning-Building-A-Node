//use bdk_wallet::Wallet as BdkWallet;
use bdk_wallet::PersistedWallet as BdkWallet;
use lightning::chain::chaininterface::{BroadcasterInterface, ConfirmationTarget, FeeEstimator};
use std::ops::Deref;
use lightning::util::logger::Logger;
use std::sync::{Mutex, RwLock};
use bdk_wallet::{AddressInfo, Balance};
use bdk_chain::ChainPosition::{Unconfirmed, Confirmed};
use lightning::log_info;
use bitcoin::blockdata::transaction::Transaction;
use bitcoin::blockdata::script::ScriptBuf;
use std::collections::HashMap;
use bitcoin::FeeRate;
use bitcoin::address::Address;
use lightning::events::bump_transaction::{Utxo, WalletSource};
use bitcoin::{OutPoint, TxOut, WPubkeyHash};
use bdk_bitcoind_rpc::{
    bitcoincore_rpc::{Auth, Client, RpcApi},
    Emitter, MempoolEvent,
};
use bitcoin::hashes::Hash;
use bitcoin::key::XOnlyPublicKey;
use bitcoin::psbt::Psbt;
use bitcoin::blockdata::constants::WITNESS_SCALE_FACTOR;
use bdk_wallet::rusqlite::Connection;
use bdk_wallet::{
    bitcoin::{Block, Network},
    KeychainKind, Wallet, SignOptions
};
use std::{
    path::PathBuf,
    sync::{mpsc::sync_channel, Arc},
    thread::spawn,
    time::Instant,
};
use lightning::sign::ChangeDestinationSource;

#[derive(Debug)]
enum Emission {
    SigTerm,
    Block(bdk_bitcoind_rpc::BlockEvent<Block>),
    Mempool(MempoolEvent),
}

pub(crate) struct OnChainWallet<B: Deref, E: Deref, L: Deref>
where
    B::Target: BroadcasterInterface,
	E::Target: FeeEstimator,
	L::Target: Logger,
{
	// A BDK on-chain wallet.
	inner: Mutex<BdkWallet<Connection>>,
    rpc_client: Arc<Client>,
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
        wallet: BdkWallet<Connection>,
        host: String,
        port: u16,
        rpc_user: String,
        path_to_db: String,
        rpc_password: String,
        fee_estimator: E,
        broadcaster: B, 
        logger: L,
    ) -> Self {
        
        let inner = Mutex::new(wallet);

        let rpc_url = format!("http://{}:{}", host, port);
        let auth = Auth::UserPass(rpc_user, rpc_password);

        let rpc_client = Arc::new(
            Client::new(&rpc_url, auth).expect("Failed to create Bitcoin Core RPC client"),
        );

        Self {
            inner,
            rpc_client,
            path_to_db,
            broadcaster,
            fee_estimator,
            logger,
        }
    }

    pub fn sync_wallet(&self) -> anyhow::Result<(),  Box<dyn std::error::Error>> {
        let mut wallet = self.inner.lock().unwrap();
        let start_load_wallet = Instant::now();
        let mut db = Connection::open(self.path_to_db.clone())?;

        let wallet_tip = wallet.latest_checkpoint();
        println!(
            "Wallet tip: {} at height {}",
            wallet_tip.hash(),
            wallet_tip.height()
        );
    
        let (sender, receiver) = sync_channel::<Emission>(21);
    
        let signal_sender = sender.clone();
        let _ = ctrlc::set_handler(move || {
            signal_sender
                .send(Emission::SigTerm)
                .expect("failed to send sigterm")
        });
    
        let rpc_client = Arc::clone(&self.rpc_client);
        let wallet_tip_clone = wallet_tip.clone();

        let mut emitter = Emitter::new(
            rpc_client,
            wallet_tip_clone,
            0,
            wallet
                .transactions()
                .filter(|tx| tx.chain_position.is_unconfirmed()),
        );

        // Move the Arc into the thread, not borrow it
        spawn(move || -> Result<(), anyhow::Error> {
            while let Some(emission) = emitter.next_block()? {
                sender.send(Emission::Block(emission))?;
            }
            sender.send(Emission::Mempool(emitter.mempool()?))?;
            Ok(())
        });
    
        let mut blocks_received = 0_usize;
        for emission in receiver {
            match emission {
                Emission::SigTerm => {
                    println!("Sigterm received, exiting...");
                    break;
                }
                Emission::Block(block_emission) => {
                    blocks_received += 1;
                    let height = block_emission.block_height();
                    let hash = block_emission.block_hash();
                    let connected_to = block_emission.connected_to();
                    let start_apply_block = Instant::now();
                    wallet.apply_block_connected_to(&block_emission.block, height, connected_to)?;
                    wallet.persist(&mut db)?;
                    //(&mut *wallet).persist(&mut db)?;
                    let elapsed = start_apply_block.elapsed().as_secs_f32();
                    println!("Applied block {hash} at height {height} in {elapsed}s");
                }
                Emission::Mempool(event) => {
                    let start_apply_mempool = Instant::now();
                    wallet.apply_evicted_txs(event.evicted);
                    wallet.apply_unconfirmed_txs(event.update);
                    wallet.persist(&mut db)?;
                    //(&mut *wallet).persist(&mut db)?;
                    println!(
                        "Applied unconfirmed transactions in {}s",
                        start_apply_mempool.elapsed().as_secs_f32()
                    );
                    break;
                }
            }
        }

        Ok(())
    }

    pub fn create_transaction(&self, outputs: Vec<HashMap<ScriptBuf, u64>>) -> Transaction {
        
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
        let fee_rate = self.fee_estimator.get_est_sat_per_1000_weight(ConfirmationTarget::NonAnchorChannelFee);
        let fee_rate_vb = fee_rate as u64 / 4; // Convert sat/kw to sat/vB
        tx_builder.fee_rate(FeeRate::from_sat_per_vb(fee_rate_vb).unwrap());

        // build the transaction
        let mut psbt = tx_builder.finish().unwrap();

        let sign_options = SignOptions {
            trust_witness_utxo: true,
            ..Default::default()
        };

        wallet.sign(&mut psbt, sign_options).unwrap();

        let tx: Transaction = psbt.extract_tx().unwrap();

        tx

    }

    pub async fn get_address(&self) -> Address {
        let mut locked_wallet = self.inner.lock().unwrap();

        let address_info = locked_wallet.reveal_next_address(KeychainKind::External);

        address_info.address
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
                let outpoint = OutPoint {
                    txid: utxo.outpoint.txid,
                    vout: utxo.outpoint.vout,
                };
                let value = bitcoin::Amount::from_sat(utxo.txout.value.to_sat());
                let address = Address::from_script(&utxo.txout.script_pubkey, Network::Regtest).ok()?;

                match address.witness_program() {
                    Some(prog) if prog.is_p2wpkh() => {
                        WPubkeyHash::from_slice(prog.program().as_bytes())
                            .map(|wpkh| Utxo::new_v0_p2wpkh(outpoint, value, &wpkh))
                            .ok()
                    }
                    Some(prog) if prog.is_p2tr() => {
                        XOnlyPublicKey::from_slice(prog.program().as_bytes())
                            .map(|_| Utxo {
                                outpoint,
                                output: TxOut{
                                    value,
                                    script_pubkey: utxo.txout.script_pubkey
                                },
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

        let mut wallet = self.inner.lock().unwrap();
        
        let sign_options = SignOptions {
            trust_witness_utxo: true,
            ..Default::default()
        };

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