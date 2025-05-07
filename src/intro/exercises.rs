use bitcoin::{TxIn};
use hex_lit::hex;
use crate::hex_utils::hex_str;

#[derive(Debug, Clone)]
pub struct Script(pub Vec<u8>);

//
// EXERCISE 1
//

#[derive(Debug, Clone)]
pub struct TxOut{
      pub amount: u64,
      pub script: Script,
}

//
// EXERCISE 2
//
pub fn sum_outputs(outputs: Vec<TxOut>) -> u64 {
      let mut total = 0;
          for tx_out in outputs {
              total += tx_out.amount;
          }
      total
}


//
// EXERCISE 3
//
pub struct Transaction {
    pub version: u8,
    pub lock_time: u32,
    pub input: Vec<TxIn>,
    pub output: Vec<TxOut>,
}

impl Transaction {
      pub fn sum_outputs(self) -> u64 {
            let mut total = 0;
                for tx_out in self.output {
                    total += tx_out.amount;
                }
            total
      }
}

//
// EXERCISE 4
//

fn main() {

    let version_bits = String::from("02000000");

    let bip68_compatible = is_bip68_compatible(version_bits);

    println!("Is {:?} BIP 68 compatible? {:?}",
            version_bits, bip68_compatible);
}

fn is_bip68_compatible(version: String) -> bool {

    if version[0..2]  == String::from("01") {
        return false;
    }
    true
}