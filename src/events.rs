#![allow(dead_code, unused_imports, unused_variables, unused_must_use)]
use crate::internal::bitcoind_client::{BitcoindClient};
use crate::internal::channel_manager::{ChannelManager};
use lightning::events::{Event};
use bitcoin_bech32::WitnessProgram;
use bitcoin_bech32::constants::Network::Regtest;
use std::collections::HashMap;
use bitcoin::blockdata::transaction::Transaction;
use bitcoin::consensus::{encode, Decodable, Encodable};
use crate::hex_utils;
use crate::internal::types::{KeysManager, PeerManager, FileStore};

pub async fn handle_ldk_events(
    channel_manager: &ChannelManager, 
    bitcoind_client: BitcoindClient,
    on_chain_wallet: 
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
            
            let confirmation_target = ConfirmationTarget::AnchorChannelFee;

            // We set nLockTime to the current height to discourage fee sniping.
            let cur_height = channel_manager.current_best_block().height;
            let locktime = LockTime::from_height(cur_height).unwrap_or(LockTime::ZERO);

            let channel_amount = Amount::from_sat(channel_value_satoshis);

            let final_tx: Transaction =
                on_chain_wallet.create_funding_transaction(
                output_script,
                channel_amount,
                confirmation_target,
                locktime);

            // Give the funding transaction back to LDK for opening the channel.
            channel_manager.funding_transaction_generated(
                temporary_channel_id,
                counterparty_node_id,
                signed_tx)
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