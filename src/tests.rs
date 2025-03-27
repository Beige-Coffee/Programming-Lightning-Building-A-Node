use crate::bitcoind_client::BitcoindClient;
use crate::hex_utils;
use bitcoin::Network;
use std::sync::Arc;
use crate::disk::FilesystemLogger;
use lightning_block_sync::{AsyncBlockSourceResult, BlockData, BlockHeaderData, BlockSource};
use hex_lit::hex;
use bitcoin::consensus::encode::{serialize_hex, deserialize};
use bitcoin::consensus::{encode, Decodable, Encodable};
use lightning::chain::chaininterface::{BroadcasterInterface, ConfirmationTarget, FeeEstimator};
use std::collections::HashMap;
use super::*;
use std::path::Path;
use std::sync::atomic::Ordering;
use tokio::net::TcpStream;
use tokio::time::{timeout, Duration};
use crate::networking::MockPeerManager;
use crate::networking::start_network_listener;
use crate::internal::channel_manager::ChannelManager;
use crate::internal::bitcoind_client::BitcoindClient as BitcoinClient;
use crate::events::handle_ldk_events;
use lightning::ln::types::ChannelId;
use bitcoin::secp256k1::{self, Secp256k1, PublicKey};
use bitcoin::{ScriptBuf};
use bitcoin::script::Builder;
use lightning::events::{Event};
use crate::internal::types::{KeysManager, PeerManager, FileStore};
use crate::commands::{open_channel};

async fn get_bitcoind_client() -> BitcoindClient {
    let logger = Arc::new(FilesystemLogger::new("test_dir".to_string()));
    let client = BitcoindClient::new(
        "localhost".to_string(),
        18443,
        "bitcoind".to_string(),
        "bitcoind".to_string(),
        Network::Regtest,
        tokio::runtime::Handle::current(),
        logger,
    ).await.unwrap();

    client
}

#[cfg(test)]
mod bitcoind_tests {
    use super::*;
    
    #[tokio::test]
    async fn test_bitcoind_client_creation() {
        let client = get_bitcoind_client().await;
        let blockchain_info = client.get_blockchain_info().await.chain;
        
        assert_eq!(
            blockchain_info,
            "regtest"
        );
    }

#[tokio::test]
    async fn test_block_source() {
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
    
        assert_eq!(
            best_block_user,
            best_block_answer
        );
    
        assert_eq!(
            header_user,
            header_answer
        );
    
        assert_eq!(
            best_header_user,
            best_header_answer
        );
    }


    #[tokio::test]
    async fn test_broadcast() {

        let client = get_bitcoind_client().await;

        let tx = "0100000001d611ad58b2f5bc0db7d15dfde4f497d6482d1b4a1e8c462ef077d4d32b3dae7901000000da0047304402203b17b4f64fa7299e8a85a688bda3cb1394b80262598bbdffd71dab1d7f266098022019cc20dc20eae417374609cb9ca22b28261511150ed69d39664b9d3b1bcb3d1201483045022100cfff9c400abb4ce5f247bd1c582cf54ec841719b0d39550b714c3c793fb4347b02201427a961a7f32aba4eeb1b71b080ea8712705e77323b747c03c8f5dbdda1025a01475221032d7306898e980c66aefdfb6b377eaf71597c449bf9ce741a3380c5646354f6de2103e8c742e1f283ef810c1cd0c8875e5c2998a05fc5b23c30160d3d33add7af565752aeffffffff020ed000000000000016001477800cff52bd58133b895622fd1220d9e2b47a79cd0902000000000017a914da55145ca5c56ba01f1b0b98d896425aa4b0f4468700000000";

        let transaction = encode::deserialize(&hex_utils::to_vec(&tx).unwrap()).unwrap();

        client.broadcast_transactions(&[&transaction]);

        tokio::time::sleep(Duration::from_millis(250)).await;

    }

    #[tokio::test]
    async fn test_fees() {

        let client = get_bitcoind_client().await;

        //println!("fees: {:?}", client.fees);

        let feerate_target = ConfirmationTarget::MaximumFeeEstimate;

        let fees = client.get_est_sat_per_1000_weight(feerate_target);

        assert_eq!(
            fees,
            50000
        );

    }

    #[tokio::test]
    async fn test_create_raw_transaction() {

        let client = get_bitcoind_client().await;

        let addr = client.get_new_address().await;

        let mut outputs = vec![HashMap::with_capacity(1)];
        outputs[0].insert(addr.to_string(), 500_000.0 / 100_000_000.0);

        let raw_tx = client.create_raw_transaction(outputs).await;

        assert_eq!(
            raw_tx.0.len(),
            82
        );

    }

    #[tokio::test]
    async fn test_start_listener() {
        // Setup
        let listening_port = 9735; // Fixed port instead of 0
        let peer_manager = Arc::new(MockPeerManager::new());
        let stop_listen = Arc::new(AtomicBool::new(false));

        // Start the listener
        start_network_listener(peer_manager.clone(), listening_port, stop_listen.clone()).await;

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
    async fn test_handle_ldk_events() {
        let channel_manager = ChannelManager::new();
        let bitcoin_client = BitcoinClient::new();
        let keys_manager = KeysManager::new();
        let peer_manager = PeerManager::new();
        let file_store = FileStore::new();

        let temporary_channel_id = ChannelId::new_zero();
        let user_channel_id = 0;
        let counterparty_node_id = pubkey_from_private_key(&[0x01; 32]);
        let channel_value_satoshis = 1_000_000;
        let output_script = Builder::new()
                .push_int(16)
                .push_slice(&[0, 40])
                .into_script();

        let event = Event::FundingGenerationReady {
            temporary_channel_id,
            counterparty_node_id,
            channel_value_satoshis,
            output_script,
            user_channel_id
            
        };


        handle_ldk_events(&channel_manager, bitcoin_client, keys_manager, peer_manager, file_store, event).await;

        // Check final state
        let final_funding_tx = channel_manager.funding_tx.lock().unwrap();
        println!("Final funding_tx: {:?}", final_funding_tx);

        assert!(final_funding_tx.is_some(), "Funding transaction should be present");

        
    }

    #[tokio::test]
    async fn test_open_channel() {
        let channel_manager = ChannelManager::new();
        let peer_pubkey = pubkey_from_private_key(&[0x01; 32]);
        let channel_amt_sat = 1_000_000;
        let announce_for_forwarding = false;
        let with_anchors = true;


        open_channel(peer_pubkey, channel_amt_sat, announce_for_forwarding, with_anchors, &channel_manager);

        // Check final state
        let final_channels = channel_manager.channels.lock().unwrap();
        println!("Final channels: {:?}", final_channels);

        assert!(final_channels.is_some(), "New channel should be present");


    }
}

fn pubkey_from_private_key(private_key: &[u8; 32]) -> PublicKey {
    let secp = Secp256k1::new();
    let secret_key = secp256k1::SecretKey::from_slice(private_key).unwrap();
    secp256k1::PublicKey::from_secret_key(&secp, &secret_key)
}