#![allow(dead_code, unused_imports, unused_variables, unused_must_use, unexpected_cfgs)]
use crate::{cli, InboundPaymentInfoStorage, NetworkGraph, OutboundPaymentInfoStorage};
use bitcoin::secp256k1::PublicKey;
use bitcoin::Network;
use chrono::Utc;
use lightning::routing::scoring::{ProbabilisticScorer, ProbabilisticScoringDecayParameters};
use lightning::util::logger::{Logger, Record};
use lightning::util::ser::{Readable, ReadableArgs};
use std::collections::HashMap;
use std::fs;
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

// define FilesystemLogger
pub(crate) struct FilesystemLogger {
  pub data_dir: String,
}

impl FilesystemLogger {
  pub(crate) fn new(data_dir: String) -> Self {
    unimplemented!()
  }
}

impl Logger for FilesystemLogger {
  fn log(&self, record: Record) {
    unimplemented!()
  }
}