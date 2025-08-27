use bdk_wallet::Wallet as BdkWallet;
use lightning::chain::chaininterface::{BroadcasterInterface, ConfirmationTarget, FeeEstimator};
use std::ops::Deref;
use lightning::util::logger::Logger;
use std::sync::{Mutex, RwLock};
use bdk_wallet::{AddressInfo, Balance};
use bdk_chain::ChainPosition::Unconfirmed;
use lightning::log_info;

use bdk_bitcoind_rpc::{
    bitcoincore_rpc::{Auth, Client, RpcApi},
    Emitter, MempoolEvent,
};
use bdk_wallet::rusqlite::Connection;
use bdk_wallet::{
    bitcoin::{Block, Network},
    KeychainKind, Wallet,
};
use std::{
    path::PathBuf,
    sync::{mpsc::sync_channel, Arc},
    thread::spawn,
    time::Instant,
};

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
	inner: Mutex<BdkWallet>,
    rpc_client: Client,
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
        wallet: BdkWallet,
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

        let rpc_client = Client::new(&rpc_url, auth)
            .expect("Failed to create Bitcoin Core RPC client");

        Self {
            inner,
            rpc_client,
            path_to_db,
            broadcaster,
            fee_estimator,
            logger,
        }
    }

    pub fn sync_wallet(&self) -> Result<(),  Box<dyn std::error::Error>> {
        let wallet = self.inner.lock().unwrap();
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
    
        let mut emitter = Emitter::new(
            &self.rpc_client,
            wallet_tip.clone(),
            wallet_tip.height(),
            {
                let wallet = self.inner.lock().unwrap();
                wallet
                    .transactions()
                    .filter(|tx| matches!(tx.chain_position, bdk_chain::ChainPosition::Unconfirmed { .. }))
                    .map(|tx| tx.tx_node.tx.clone())
                    .collect::<Vec<_>>()
            }
        );

        spawn(move || -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
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
                    
                    // Lock the wallet, apply block, and persist
                    {
                        let mut wallet = self.inner.lock().unwrap();
                        wallet.apply_block_connected_to(&block_emission.block, height, connected_to)?;
                    }
                    
                    let elapsed = start_apply_block.elapsed().as_secs_f32();
                    println!("Applied block {hash} at height {height} in {elapsed}s");
                }
                Emission::Mempool(event) => {
                    let start_apply_mempool = Instant::now();
                    
                    // Lock the wallet, apply mempool changes, and persist
                    {
                        let mut wallet = self.inner.lock().unwrap();
                        wallet.apply_unconfirmed_txs(event.update);
                    }
                    
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
}

