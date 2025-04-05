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