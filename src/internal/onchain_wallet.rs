use crate::intro::exercises::Script;
use bitcoin::{Amount, locktime::absolute::LockTime};
use lightning::chain::chaininterface::ConfirmationTarget;

struct MockWallet;

impl MockWallet {
    fn create_funding_transaction(
        &self,
        output_script: Script,
        channel_amount: Amount,
        confirmation_target: ConfirmationTarget,
        locktime: LockTime,
    ) -> String {
        // Creating a fake transaction as a string for mock purposes
        let fake_tx = format!(
            "FakeTx -- Script: {:?}, Amount: {:?}, Confirmation Target: {:?}, Locktime: {:?}",
            output_script, channel_amount, confirmation_target, locktime
        );

        fake_tx
    }
}