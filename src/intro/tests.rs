use crate::intro::exercises::{TxOut, Script, sum_outputs};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_struct_tx_out() {
        let amount: u64 = 10;
        let script_bytes = Script(vec![0x76, 0xa9, 0x14]);
        
        // Attempt to create TxOut and handle potential errors
        let result = std::panic::catch_unwind(|| {
            TxOut {
                amount,
                script: script_bytes.clone(),
            }
        });

        // Check if the struct was created successfully
        if result.is_err() {
            // Suppress error details and fail the test with a custom message
            panic!("Failed to create TxOut: ensure the struct is defined with `amount: u64` and `script: Vec<u8>`");
        }

        // If creation succeeded, extract the TxOut instance
        let tx_out = result.unwrap();
    }

    #[test]
    fn test_sum_outputs() {

        let amount: u64 = 10;
        let script_bytes = Script(vec![0x76, 0xa9, 0x14]);

        let tx_out = TxOut {
                amount,
                script: script_bytes.clone(),
            };

        let sum = sum_outputs(vec![tx_out.clone(),tx_out.clone() ]);
        
        assert_eq!(
            sum,
            20
        );
    }
}