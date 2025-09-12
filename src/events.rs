#![allow(dead_code, unused_imports, unused_variables, unused_must_use)]
use crate::internal::bitcoind_client::{BitcoindClient};
use crate::internal::channel_manager::{ChannelManager};
use lightning::chain::chaininterface::{BroadcasterInterface, ConfirmationTarget, FeeEstimator};
use lightning::events::{Event};
use bitcoin_bech32::WitnessProgram;
use bitcoin_bech32::constants::Network::Regtest;
use std::collections::HashMap;
use bitcoin::blockdata::transaction::Transaction;
use bitcoin::consensus::{encode, Decodable, Encodable};
use crate::hex_utils;
use crate::internal::types::{KeysManager, PeerManager, FileStore};
use crate::LdkOnChainWallet as OnChainWallet;
use bitcoin::blockdata::locktime::absolute::LockTime;
use bitcoin::{
    Amount,
};

////////////////////////////
// START Exercise 10 //
// Implement FundingGenerationReady for handle_ldk_events
////////////////////////////

pub async fn handle_ldk_events(
    channel_manager: &ChannelManager, 
    bitcoind_client: BitcoindClient,
    on_chain_wallet: &OnChainWallet,
    keys_manager: KeysManager, 
    peer_manager: PeerManager,
    file_store: FileStore,
    event: Event
) {
    match event {
        Event::FundingGenerationReady {
            temporary_channel_id,
            counterparty_node_id,
            channel_value_satoshis,
            output_script,
            ..
        } => {
            
            let confirmation_target = ConfirmationTarget::NonAnchorChannelFee;

            // We set nLockTime to the current height to discourage fee sniping.
            let cur_height = channel_manager.current_best_block().height;
            let locktime = LockTime::from_height(cur_height).unwrap();

            let channel_amount = Amount::from_sat(channel_value_satoshis);

            let final_tx = on_chain_wallet.create_funding_transaction(
                output_script,
                channel_amount,
                confirmation_target,
                locktime);

            // Give the funding transaction back to LDK for opening the channel.
            channel_manager.funding_transaction_generated(
                temporary_channel_id,
                counterparty_node_id,
                final_tx)
        },
        Event::FundingTxBroadcastSafe { .. } => {},
        Event::PaymentClaimable { .. } => {},
        Event::PendingHTLCsForwardable { .. } => {},
        Event::SpendableOutputs { .. } => {},
        Event::ChannelReady { .. } => {},
        Event::ChannelClosed { .. } => {},
        _ => {},
    }
}