use bitcoin::blockdata::script::ScriptBuf;
use bitcoin::{
	Amount,
};
use lightning::chain::chaininterface::{BroadcasterInterface, ConfirmationTarget, FeeEstimator};
use bitcoin::blockdata::locktime::absolute::LockTime;

pub struct KeysManager(String);

impl KeysManager {
	pub fn new() -> Self {
		KeysManager(String::new())
	}
}

pub struct PeerManager(String);

impl PeerManager {
	pub fn new() -> Self {
		PeerManager(String::new())
	}
}

pub struct FileStore(String);

impl FileStore {
	pub fn new() -> Self {
		FileStore(String::new())
	}
	pub fn write(&self, primary_namespace: &str, secondary_namespace: &str, key: &str, buf: &[u8]) -> Result<(), std::io::Error>{
		Ok(())
	}
}

pub struct OnChainWallet(String);

impl OnChainWallet {
	pub fn new() -> Self {
		OnChainWallet(String::new())
	}
	
	pub fn create_funding_transaction(&self,
		output_script: ScriptBuf,
		amount: Amount,
		confirmation_target: ConfirmationTarget,
		locktime: LockTime) -> String {
		"010000000001013c735f81c1a0115af2e735554fb271ace18c32a3faf443f9db40cb9a11ca63110000000000ffffffff02b113030000000000160014689a681c462536ad7d735b497511e527e9f59245cf120000000000001600148859f1e9ef3ba438e2ec317f8524ed41f8f06c6a024730440220424772d4ad659960d4f1b541fd853f7da62e8cf505c2f16585dc7c8cf643fe9a02207fbc63b9cf317fc41402b2e7f6fdc1b01f1b43c5456cf9b547fe9645a16dcb150121032533cb19cf37842556dd2168b1c7b6f3a70cff25a6ff4d4b76f2889d2c88a3f200000000".to_string()
}}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RetryableSendFailure {
	/// The provided [`PaymentParameters::expiry_time`] indicated that the payment has expired. Note
	/// that this error is *not* caused by [`Retry::Timeout`].
	///
	/// [`PaymentParameters::expiry_time`]: crate::routing::router::PaymentParameters::expiry_time
	PaymentExpired,
	/// We were unable to find a route to the destination.
	RouteNotFound,
	/// Indicates that a payment for the provided [`PaymentId`] is already in-flight and has not
	/// yet completed (i.e. generated an [`Event::PaymentSent`] or [`Event::PaymentFailed`]).
	///
	/// [`PaymentId`]: crate::ln::channelmanager::PaymentId
	/// [`Event::PaymentSent`]: crate::events::Event::PaymentSent
	/// [`Event::PaymentFailed`]: crate::events::Event::PaymentFailed
	DuplicatePayment,
	/// The [`RecipientOnionFields::payment_metadata`], [`RecipientOnionFields::custom_tlvs`], or
	/// [`BlindedPaymentPath`]s provided are too large and caused us to exceed the maximum onion
	/// packet size of 1300 bytes.
	///
	/// [`BlindedPaymentPath`]: crate::blinded_path::payment::BlindedPaymentPath
	OnionPacketSizeExceeded,
}