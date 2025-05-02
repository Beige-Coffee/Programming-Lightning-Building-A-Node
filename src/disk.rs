#![allow(dead_code, unused_imports, unused_variables, unused_must_use, unexpected_cfgs)]
use crate::{cli, InboundPaymentInfoStorage, NetworkGraph, OutboundPaymentInfoStorage};
use crate::logger::FilesystemLogger;
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


pub(crate) const INBOUND_PAYMENTS_FNAME: &str = "inbound_payments";
pub(crate) const OUTBOUND_PAYMENTS_FNAME: &str = "outbound_payments";

pub(crate) fn persist_channel_peer(path: &Path, peer_info: &str) -> std::io::Result<()> {
	let mut file = fs::OpenOptions::new().create(true).append(true).open(path)?;
	file.write_all(format!("{}\n", peer_info).as_bytes())
}

pub(crate) fn read_channel_peer_data(
	path: &Path,
) -> Result<HashMap<PublicKey, SocketAddr>, std::io::Error> {
	let mut peer_data = HashMap::new();
	if !Path::new(&path).exists() {
		return Ok(HashMap::new());
	}
	let file = File::open(path)?;
	let reader = BufReader::new(file);
	for line in reader.lines() {
		match cli::parse_peer_info(line.unwrap()) {
			Ok((pubkey, socket_addr)) => {
				peer_data.insert(pubkey, socket_addr);
			},
			Err(e) => return Err(e),
		}
	}
	Ok(peer_data)
}

pub(crate) fn read_network(
	path: &Path, network: Network, logger: Arc<FilesystemLogger>,
) -> NetworkGraph {
	if let Ok(file) = File::open(path) {
		if let Ok(graph) = NetworkGraph::read(&mut BufReader::new(file), logger.clone()) {
			return graph;
		}
	}
	NetworkGraph::new(network, logger)
}

pub(crate) fn read_inbound_payment_info(path: &Path) -> InboundPaymentInfoStorage {
	if let Ok(file) = File::open(path) {
		if let Ok(info) = InboundPaymentInfoStorage::read(&mut BufReader::new(file)) {
			return info;
		}
	}
	InboundPaymentInfoStorage { payments: HashMap::new() }
}

pub(crate) fn read_outbound_payment_info(path: &Path) -> OutboundPaymentInfoStorage {
	if let Ok(file) = File::open(path) {
		if let Ok(info) = OutboundPaymentInfoStorage::read(&mut BufReader::new(file)) {
			return info;
		}
	}
	OutboundPaymentInfoStorage { payments: HashMap::new() }
}

pub(crate) fn read_scorer(
	path: &Path, graph: Arc<NetworkGraph>, logger: Arc<FilesystemLogger>,
) -> ProbabilisticScorer<Arc<NetworkGraph>, Arc<FilesystemLogger>> {
	let params = ProbabilisticScoringDecayParameters::default();
	if let Ok(file) = File::open(path) {
		let args = (params.clone(), Arc::clone(&graph), Arc::clone(&logger));
		if let Ok(scorer) = ProbabilisticScorer::read(&mut BufReader::new(file), args) {
			return scorer;
		}
	}
	ProbabilisticScorer::new(params, graph, logger)
}
