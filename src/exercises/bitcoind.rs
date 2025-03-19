
use crate::convert::{
    BlockchainInfo, FeeResponse, FundedTx, ListUnspentResponse, MempoolMinFeeResponse, NewAddress,
    RawTx, SignedTx,
};
use crate::disk::FilesystemLogger;
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
use serde_json;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub struct BitcoindClient {
    pub(crate) bitcoind_rpc_client: Arc<RpcClient>,
    network: Network,
    host: String,
    port: u16,
    rpc_user: String,
    rpc_password: String,
    fees: Arc<HashMap<ConfirmationTarget, AtomicU32>>,
    handle: tokio::runtime::Handle,
    logger: Arc<FilesystemLogger>,
}

impl BlockSource for BitcoindClient {
    fn get_header<'a>(
        &'a self, header_hash: &'a BlockHash, height_hint: Option<u32>,
    ) -> AsyncBlockSourceResult<'a, BlockHeaderData> {
        Box::pin(async move { self.bitcoind_rpc_client.get_header(header_hash, height_hint).await })
    }

    fn get_block<'a>(
        &'a self, header_hash: &'a BlockHash,
    ) -> AsyncBlockSourceResult<'a, BlockData> {
        Box::pin(async move { self.bitcoind_rpc_client.get_block(header_hash).await })
    }

    fn get_best_block<'a>(&'a self) -> AsyncBlockSourceResult<(BlockHash, Option<u32>)> {
        Box::pin(async move { self.bitcoind_rpc_client.get_best_block().await })
    }
}

/// The minimum feerate we are allowed to send, as specify by LDK.
pub const MIN_FEERATE: u32 = 253;

impl BitcoindClient {
    pub(crate) async fn new(
        host: String, port: u16, rpc_user: String, rpc_password: String, network: Network,
        handle: tokio::runtime::Handle, logger: Arc<FilesystemLogger>,
    ) -> std::io::Result<Self> {
        let http_endpoint = HttpEndpoint::for_host(host.clone()).with_port(port);
        let rpc_credentials =
            base64::encode(format!("{}:{}", rpc_user.clone(), rpc_password.clone()));
        let bitcoind_rpc_client = RpcClient::new(&rpc_credentials, http_endpoint)?;
        let _dummy = bitcoind_rpc_client
            .call_method::<BlockchainInfo>("getblockchaininfo", &vec![])
            .await
            .map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::PermissionDenied,
                "Failed to make initial call to bitcoind - please check your RPC user/password and access settings")
            })?;
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::consensus::encode;
    use bitcoin::hashes::Hash;
    
    #[test]
    fn test_broadcast_transaction() {
        let client = ExerciseBitcoindClient {};
        let tx = Transaction { version: 2, lock_time: 0, input: vec![], output: vec![] };
        client.broadcast_transactions(&[&tx]);
        // TODO: Students add assertions to verify broadcast
    }
}
