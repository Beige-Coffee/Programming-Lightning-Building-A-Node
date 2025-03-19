
use lightning::chain::chaininterface::{ConfirmationTarget, FeeEstimator};

pub struct ExerciseFeeEstimator {
    // TODO: Students implement fields
}

impl FeeEstimator for ExerciseFeeEstimator {
    fn get_est_sat_per_1000_weight(&self, _confirmation_target: ConfirmationTarget) -> u32 {
        // TODO: Students implement fee estimation
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_fee_estimation() {
        let estimator = ExerciseFeeEstimator {};
        assert!(estimator.get_est_sat_per_1000_weight(ConfirmationTarget::NonAnchorChannelFee) > 0);
    }
}
