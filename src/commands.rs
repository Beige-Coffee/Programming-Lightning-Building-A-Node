use crate::internal::channel_manager::ChannelManager;
use bitcoin::secp256k1::PublicKey;
use lightning::util::config::{ChannelHandshakeConfig, ChannelHandshakeLimits, UserConfig};
use lightning::types::payment::{PaymentHash, PaymentPreimage, PaymentSecret};
use lightning::ln::channelmanager::{PaymentId,
};
use std::collections::HashMap;

//
  // Open Channel Exercise
//
pub fn open_channel(
	peer_pubkey: PublicKey, channel_amt_sat: u64, announce_for_forwarding: bool,
	with_anchors: bool, channel_manager: &ChannelManager,
) -> Result<(), ()> {
  
  let config = UserConfig {
    channel_handshake_limits: ChannelHandshakeLimits {
      // lnd's max to_self_delay is 2016, so we want to be compatible.
      their_to_self_delay: 2016,
      ..Default::default()
    },
    channel_handshake_config: ChannelHandshakeConfig {
      announce_for_forwarding,
      negotiate_anchors_zero_fee_htlc_tx: with_anchors,
      ..Default::default()
    },
    ..Default::default()
  };

  match channel_manager.create_channel(peer_pubkey, channel_amt_sat, 0, 0, None, Some(config)) {
    Ok(_) => {
      println!("EVENT: initiated channel with peer {}. ", peer_pubkey);
      return Ok(());
    },
    Err(e) => {
      println!("ERROR: failed to open channel: {:?}", e);
      return Err(());
    },
  }
}

//
  // Send Payment Exercise 1
//

#[derive(Copy, Clone, Debug)]
pub(crate) enum HTLCStatus {
  Pending,
  Succeeded,
  Failed,
}

pub(crate) struct MillisatAmount(pub Option<u64>);

pub(crate) struct PaymentInfo {
  pub preimage: Option<PaymentPreimage>,
  pub secret: Option<PaymentSecret>,
  pub status: HTLCStatus,
  pub amt_msat: MillisatAmount,
}

pub(crate) struct OutboundPaymentInfoStorage {
  pub payments: HashMap<PaymentId, PaymentInfo>,
}

//
  // Send Payment Exercise 2
//