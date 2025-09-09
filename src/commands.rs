use crate::internal::channel_manager::ChannelManager;
use crate::internal::types::FileStore;
use bitcoin::secp256k1::PublicKey;
use lightning::util::config::{ChannelHandshakeConfig, ChannelHandshakeLimits, UserConfig};
use lightning::types::payment::{PaymentHash, PaymentPreimage, PaymentSecret};
use lightning::ln::channelmanager::{PaymentId, Retry};
use std::collections::HashMap;
use lightning_invoice::{Bolt11Invoice};
use lightning::ln::bolt11_payment::payment_parameters_from_invoice;
use bitcoin::hashes::sha256::Hash as Sha256;
use bitcoin::hashes::Hash;
use lightning::{impl_writeable_tlv_based};
use std::time::Duration;
//
  // Open Channel Exercise
//
pub fn open_channel(
	peer_pubkey: PublicKey, channel_amt_sat: u64, announce_channel: bool,
	with_anchors: bool, channel_manager: &ChannelManager,
) -> Result<(), ()> {
  
  let config = UserConfig {
    channel_handshake_limits: ChannelHandshakeLimits::default(),
    channel_handshake_config: ChannelHandshakeConfig {
      announce_for_forwarding: announce_channel,
      their_channel_reserve_proportional_millionths: 1_000,
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

impl OutboundPaymentInfoStorage {
  fn encode(&self) -> [u8; 32] {
    [0x01; 32]
  }
}

//
  // Send Payment Exercise 2
//

pub(crate) const OUTBOUND_PAYMENTS_FNAME: &str = "outbound_payments";

pub fn send_payment(
  channel_manager: &ChannelManager, invoice: &Bolt11Invoice, required_amount_msat: Option<u64>,
  outbound_payments: &mut OutboundPaymentInfoStorage, fs_store: FileStore,
) { 

  let payment_id = PaymentId((*invoice.payment_hash()).to_byte_array());
  let payment_secret = Some(*invoice.payment_secret());

  let pay_params_opt = payment_parameters_from_invoice(invoice);
  
  let (payment_hash, recipient_onion, route_params) = match pay_params_opt {
    Ok(res) => res,
    Err(e) => {
      println!("Failed to parse invoice: {:?}", e);
      print!("> ");
      return;
    },
  };
  outbound_payments.payments.insert(
    payment_id,
    PaymentInfo {
      preimage: None,
      secret: payment_secret,
      status: HTLCStatus::Pending,
      amt_msat: MillisatAmount(invoice.amount_milli_satoshis()),
    },
  );
  fs_store.write("", "", OUTBOUND_PAYMENTS_FNAME, &outbound_payments.encode()).unwrap();

  match channel_manager.send_payment(
    payment_hash,
    recipient_onion,
    payment_id,
    route_params,
    Retry::Timeout(Duration::from_secs(10)),
  ) {
    Ok(_) => {
      let payee_pubkey = invoice.recover_payee_pub_key();
      let amt_msat = invoice.amount_milli_satoshis().unwrap();
      println!("EVENT: initiated sending {} msats to {}", amt_msat, payee_pubkey);
      print!("> ");
    },
    Err(e) => {
      println!("ERROR: failed to send payment: {:?}", e);
      print!("> ");
      outbound_payments.payments.get_mut(&payment_id).unwrap().status = HTLCStatus::Failed;
      fs_store.write("", "", OUTBOUND_PAYMENTS_FNAME, &outbound_payments.encode()).unwrap();
    },
  };
  
}