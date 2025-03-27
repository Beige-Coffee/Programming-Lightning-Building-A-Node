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
            // Construct the raw transaction with one output, that is paid the amount of the
            // channel.
            let addr = WitnessProgram::from_scriptpubkey(
                &output_script.as_bytes(),
                Regtest)
                .expect("Lightning funding tx should always be to a SegWit output")
                .to_address();
            
            let mut outputs = vec![HashMap::with_capacity(1)];
            outputs[0].insert(addr, channel_value_satoshis as f64 / 100_000_000.0);
            
            let raw_tx = bitcoind_client.create_raw_transaction(outputs).await;

            // Have your wallet put the inputs into the transaction such that the output is
            // satisfied.
            let funded_tx = bitcoind_client.fund_raw_transaction(raw_tx).await;

            // Sign the final funding transaction and give it to LDK, who will eventually broadcast it.
            let signed_tx = bitcoind_client.sign_raw_transaction_with_wallet(funded_tx).await;

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