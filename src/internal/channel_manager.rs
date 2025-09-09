use std::sync::Mutex;
use lightning::ln::types::ChannelId;
use bitcoin::secp256k1::PublicKey;
use lightning::util::config::{ChannelHandshakeConfig, ChannelHandshakeLimits, UserConfig};
use std::io::Error;
use lightning::ln::channelmanager::{PaymentId, RecipientOnionFields, Retry};
use lightning::types::payment::{PaymentHash, PaymentPreimage, PaymentSecret};
use lightning::routing::router::{RouteParameters};
use bitcoin::blockdata::transaction::Transaction;

pub struct BestBlock {
    pub block_hash: String,
    pub height: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RetryableSendFailure {
  PaymentExpired,
  RouteNotFound,
  DuplicatePayment,
  OnionPacketSizeExceeded,
}

pub struct ChannelManager {
  pub funding_tx: Mutex<Option<(ChannelId, PublicKey, Transaction)>>,
  pub channel: Mutex<Option<(u128, u64, UserConfig)>>,
  pub payment: Mutex<Option<PaymentId>>,
}

impl ChannelManager {
  pub fn new() -> Self {
    Self {
      funding_tx: Mutex::new(None),
      channel: Mutex::new(None),
      payment: Mutex::new(None)
    }
  }

  pub fn funding_transaction_generated(
    &self,
    temporary_channel_id: ChannelId,
    counterparty_node_id: PublicKey,
    funding_transaction: Transaction,
  ) {
    let mut funding_tx = self.funding_tx.lock().unwrap();

    *funding_tx = Some((
      temporary_channel_id,
      counterparty_node_id.clone(),
      funding_transaction,
    ));
  }

  pub fn create_channel(&self, their_network_key: PublicKey, channel_value_satoshis: u64, push_msat: u64, user_channel_id: u128, temporary_channel_id: Option<ChannelId>, override_config: Option<UserConfig>) -> Result<ChannelId, Error> {
    let mut channels = self.channel.lock().unwrap();

    *channels = Some((
      user_channel_id,
      channel_value_satoshis,
      override_config.unwrap_or_default()
    ));

    let chan_id = ChannelId::new_zero();

    Ok(chan_id)
  }

  pub fn current_best_block(&self) -> BestBlock {
    BestBlock {
      block_hash: "000".to_string(),
      height: 300
    }
  }

  pub fn send_payment(&self, payment_hash: PaymentHash, recipient_onion: RecipientOnionFields, payment_id: PaymentId, route_params: RouteParameters, retry_strategy: Retry) -> Result<(), RetryableSendFailure> {
    
    let mut payments = self.payment.lock().unwrap();

    *payments = Some(
      payment_id,
    );

    Ok(())
  }
}