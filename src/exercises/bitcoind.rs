
use bitcoin::blockdata::transaction::Transaction;
use bitcoin::Address;
use bitcoin::Network;
use lightning::chain::chaininterface::BroadcasterInterface;

pub struct ExerciseBitcoindClient {
    // TODO: Students implement fields for network info and RPC details
}

impl BroadcasterInterface for ExerciseBitcoindClient {
    fn broadcast_transactions(&self, txs: &[&Transaction]) {
        // TODO: Students implement transaction broadcasting
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::consensus::encode;
    use bitcoin::hashes::Hash;
    
    #[test]
    fn test_broadcast_transaction() {
        let client = ExerciseBitcoindClient {};
        let tx = Transaction { version: 2, lock_time: 0, input: vec![], output: vec![] };
        client.broadcast_transactions(&[&tx]);
        // TODO: Students add assertions to verify broadcast
    }
}
