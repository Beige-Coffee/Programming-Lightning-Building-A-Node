use std::sync::Mutex;
use lightning::ln::types::ChannelId;
use bitcoin::secp256k1::PublicKey;
use lightning::util::config::{ChannelHandshakeConfig, ChannelHandshakeLimits, UserConfig};
use std::io::Error;

pub struct ChannelManager {
  pub funding_tx: Mutex<Option<(ChannelId, PublicKey, String)>>,
  pub channels: Mutex<Option<(u128, u64)>>,
}

impl ChannelManager {
  pub fn new() -> Self {
    Self {
      funding_tx: Mutex::new(None),
      channels: Mutex::new(None)
    }
  }

  pub fn funding_transaction_generated(
    &self,
    temporary_channel_id: ChannelId,
    counterparty_node_id: PublicKey,
    funding_transaction: String,
  ) {
    let mut funding_tx = self.funding_tx.lock().unwrap();

    *funding_tx = Some((
      temporary_channel_id,
      counterparty_node_id.clone(),
      funding_transaction,
    ));
  }

  pub fn create_channel(&self, their_network_key: PublicKey, channel_value_satoshis: u64, push_msat: u64, user_channel_id: u128, temporary_channel_id: Option<ChannelId>, override_config: Option<UserConfig>) -> Result<ChannelId, Error> {
    let mut channels = self.channels.lock().unwrap();

    *channels = Some((
      user_channel_id,
      channel_value_satoshis
    ));

    let chan_id = ChannelId::new_zero();

    Ok(chan_id)
  }
}