#![allow(dead_code, unused_imports, unused_variables, unused_must_use)]
use bitcoin::bip32::{ChildNumber, Xpriv, Xpub};
use bitcoin::hashes::sha256::Hash as Sha256;
use bitcoin::hashes::{Hash, HashEngine};
use bitcoin::network::Network;
use bitcoin::secp256k1;
use bitcoin::secp256k1::PublicKey;
use bitcoin::secp256k1::Scalar;
use bitcoin::secp256k1::Secp256k1;
use bitcoin::secp256k1::SecretKey;

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct NodeKeysManager {
    pub secp_ctx: Secp256k1<secp256k1::All>,
    pub channel_master_key: Xpriv,
    pub node_secret: SecretKey,
    pub node_id: PublicKey,
    pub seed: [u8; 32],
}

impl NodeKeysManager {
    pub(crate) fn new(seed: [u8; 32]) -> NodeKeysManager {
        let secp_ctx = Secp256k1::new();

        let master_key = get_master_key(seed);

        let node_secret = master_key
            .derive_priv(&secp_ctx, &ChildNumber::from_hardened_idx(0).unwrap())
            .expect("Your RNG is busted")
            .private_key;
        let node_id = PublicKey::from_secret_key(&secp_ctx, &node_secret);

        let channel_master_key = get_hardened_extended_child_private_key(master_key, 3);

        NodeKeysManager {
            secp_ctx: secp_ctx,
            channel_master_key: channel_master_key,
            node_secret: node_secret,
            node_id: node_id,
            seed: seed,
        }
    }
}

fn get_master_key(seed: [u8; 32]) -> Xpriv {
    let master_key = match Xpriv::new_master(Network::Regtest, &seed) {
        Ok(key) => key,
        Err(_) => panic!("Your RNG is busted"),
    };
    master_key
}

fn get_hardened_extended_child_private_key(master_key: Xpriv, idx: u32) -> Xpriv {
    let secp_ctx = Secp256k1::new();
    let hardened_extended_child = master_key
        .derive_priv(&secp_ctx, &ChildNumber::from_hardened_idx(idx).unwrap())
        .expect("Your RNG is busted");
    hardened_extended_child
}