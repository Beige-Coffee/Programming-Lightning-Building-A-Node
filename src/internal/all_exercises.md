#### Ex 1: Logger
```rust
pub(crate) struct FilesystemLogger {
  pub data_dir: String,
}

impl FilesystemLogger {
  pub(crate) fn new(data_dir: String) -> Self {
    let logs_path = format!("{}/logs", data_dir);
    fs::create_dir_all(logs_path.clone()).unwrap();
    Self { data_dir: logs_path }
  }
}

impl Logger for FilesystemLogger {
  fn log(&self, record: Record) {
    let raw_log = record.args.to_string();
    let log = format!(
      "{} {:<5} [{}:{}] {}\n",
      Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"),
      record.level.to_string(),
      record.module_path,
      record.line,
      raw_log
    );
    let logs_file_path = format!("{}/logs.txt", self.data_dir.clone());
    fs::OpenOptions::new()
      .create(true)
      .append(true)
      .open(logs_file_path)
      .unwrap()
      .write_all(log.as_bytes())
      .unwrap();
  }
}
```

#### Ex 2: BitcoindClient New
```rust
pub struct BitcoindClient {
  pub(crate) bitcoind_rpc_client: Arc<RpcClient>,
  network: Network,
  host: String,
  port: u16,
  rpc_user: String,
  rpc_password: String,
  pub fees: Arc<HashMap<ConfirmationTarget, AtomicU32>>,
  handle: tokio::runtime::Handle,
  logger: Arc<FilesystemLogger>,
}

impl BitcoindClient {
  pub(crate) async fn new(
    host: String, port: u16, rpc_user: String, rpc_password: String, network: Network,
    handle: tokio::runtime::Handle, logger: Arc<FilesystemLogger>,
  ) -> std::io::Result<Self> {
    let http_endpoint = HttpEndpoint::for_host(host.clone()).with_port(port);
    let rpc_credentials =
      base64::encode(format!("{}:{}", rpc_user.clone(), rpc_password.clone()));
    let bitcoind_rpc_client = RpcClient::new(&rpc_credentials, http_endpoint)?;

    let mut fees: HashMap<ConfirmationTarget, AtomicU32> = HashMap::new();

    fees.insert(ConfirmationTarget::MaximumFeeEstimate, AtomicU32::new(50000));
    fees.insert(ConfirmationTarget::UrgentOnChainSweep, AtomicU32::new(5000));
    fees.insert(
        ConfirmationTarget::MinAllowedAnchorChannelRemoteFee,
        AtomicU32::new(MIN_FEERATE),
    );
    fees.insert(
        ConfirmationTarget::MinAllowedNonAnchorChannelRemoteFee,
        AtomicU32::new(MIN_FEERATE),
    );
    fees.insert(ConfirmationTarget::AnchorChannelFee, AtomicU32::new(MIN_FEERATE));
    fees.insert(ConfirmationTarget::NonAnchorChannelFee, AtomicU32::new(2000));
    fees.insert(ConfirmationTarget::ChannelCloseMinimum, AtomicU32::new(MIN_FEERATE));
    fees.insert(ConfirmationTarget::OutputSpendingFee, AtomicU32::new(MIN_FEERATE));


    let client = Self {
      bitcoind_rpc_client: Arc::new(bitcoind_rpc_client),
      host,
      port,
      rpc_user,
      rpc_password,
      network,
      fees: Arc::new(fees),
      handle: handle.clone(),
      logger,
    };

    BitcoindClient::poll_for_fee_estimates(
        client.fees.clone(),
        client.bitcoind_rpc_client.clone(),
        handle,
    );

    Ok(client)
  }
}
```

#### Ex 3: BitcoindClient BlockSource
```rust
impl BlockSource for BitcoindClient {
  fn get_header<'a>(
    &'a self, header_hash: &'a BlockHash, height_hint: Option<u32>,
  ) -> AsyncBlockSourceResult<'a, BlockHeaderData> {
    Box::pin(async move { 
      let header_hash = serde_json::json!(header_hash.to_string());
      Ok(self.bitcoind_rpc_client.call_method("getblockheader", &[header_hash]).await?)
    })
  }

  fn get_block<'a>(
    &'a self, header_hash: &'a BlockHash,
  ) -> AsyncBlockSourceResult<'a, BlockData> {
    Box::pin( async move {
      let header_hash = serde_json::json!(header_hash.to_string());
      let verbosity = serde_json::json!(0);
      Ok(BlockData::FullBlock(self.bitcoind_rpc_client.call_method("getblock", &[header_hash, verbosity]).await?))
    })
  }

  fn get_best_block<'a>(&'a self) -> AsyncBlockSourceResult<(BlockHash, Option<u32>)> {
    Box::pin(async move {
      Ok(self.bitcoind_rpc_client.call_method("getblockchaininfo", &[]).await?)
    })
  }
}
```

#### Ex 4: BitcoindClient BroadcasterInterface
```rust
impl BroadcasterInterface for BitcoindClient {
  fn broadcast_transactions(&self, txs: &[&Transaction]) {
    let txn = txs.iter().map(|tx| encode::serialize_hex(tx)).collect::<Vec<_>>();
    for tx in txn {
      let bitcoind_rpc_client = Arc::clone(&self.bitcoind_rpc_client);
      let logger = Arc::clone(&self.logger);
      self.handle.spawn(async move {
        let tx_json = serde_json::json!(tx);
        match bitcoind_rpc_client
          .call_method::<serde_json::Value>("sendrawtransaction", &[tx_json])
          .await

        {
          Ok(result) => {
            log_info!(logger, "Successfully broadcasted transaction: {:?}", result);
          },
          Err(e) => {
            log_error!(logger, "Failed to broadcast transaction: {:?}", e);
          }
        }
      });
    }
  }
}
```


#### Ex 5: BitcoindClient FeeEstimator
```rust
impl FeeEstimator for BitcoindClient {
  fn get_est_sat_per_1000_weight(&self, confirmation_target: ConfirmationTarget) -> u32 {
    self.fees.get(&confirmation_target).unwrap().load(Ordering::Acquire)
  }
}
```

#### Ex 6: BitcoindClient On-Chain Wallet
```rust

pub async fn send_raw_transaction(&self, raw_tx: RawTx) {
  let raw_tx_json = serde_json::json!(raw_tx.0);
  self.bitcoind_rpc_client
    .call_method::<Txid>("sendrawtransaction", &[raw_tx_json])
    .await
    .unwrap();
}

pub async fn sign_raw_transaction_with_wallet(&self, tx_hex: String) -> SignedTx {
  let tx_hex_json = serde_json::json!(tx_hex);
  self.bitcoind_rpc_client
    .call_method("signrawtransactionwithwallet", &vec![tx_hex_json])
    .await
    .unwrap()
}
```

#### Ex 7: Off-Chain Wallet (KeysManager)
```rust
impl NodeKeysManager {
  pub(crate) fn new(seed: [u8; 32]) -> NodeKeysManager {
    let secp_ctx = Secp256k1::new();

    let master_key = match Xpriv::new_master(Network::Testnet, &seed) {
      Ok(key) => key,
      Err(_) => panic!("Your RNG is busted"),
    };

    let node_secret = master_key
      .derive_priv(&secp_ctx, &ChildNumber::from_hardened_idx(0).unwrap())
      .expect("Your RNG is busted")
      .private_key;

    let destination_key = master_key
      .derive_priv(&secp_ctx, &ChildNumber::from_hardened_idx(2).unwrap())
      .expect("Your RNG is busted");

    let destination_xpub = Xpub::from_priv(&secp_ctx, &destination_key);

    let node_id = PublicKey::from_secret_key(&secp_ctx, &node_secret);

    let channel_master_key = master_key
      .derive_priv(&secp_ctx, &ChildNumber::from_hardened_idx(3).unwrap())
      .expect("Your RNG is busted");

    NodeKeysManager {
      secp_ctx,
      node_secret,
      node_id,
      destination_xpub,
      channel_master_key,
      channel_child_index: 0,
      seed,
    }
  }
}
```

#### Ex 8: Persist
```rust
fn write(
  &self, primary_namespace: &str, secondary_namespace: &str, key: &str, buf: &[u8],
) -> lightning::io::Result<()> {
  check_namespace_key_validity(primary_namespace, secondary_namespace, Some(key), "write")?;

  let mut dest_file_path = self.get_dest_dir_path(primary_namespace, secondary_namespace)?;
  dest_file_path.push(key);

  let parent_directory = dest_file_path.parent().ok_or_else(|| {
    let msg =
      format!("Could not retrieve parent directory of {}.", dest_file_path.display());
    std::io::Error::new(std::io::ErrorKind::InvalidInput, msg)
  })?;
  fs::create_dir_all(&parent_directory)?;

  let mut tmp_file_path = dest_file_path.clone();
  let tmp_file_ext = format!("{}.tmp", self.tmp_file_counter.fetch_add(1, Ordering::AcqRel));
  tmp_file_path.set_extension(tmp_file_ext);

  {
    let mut tmp_file = fs::File::create(&tmp_file_path)?;
    tmp_file.write_all(&buf)?;
    tmp_file.sync_all()?;
  }

  let res = {
    let inner_lock_ref = {
      let mut outer_lock = self.locks.lock().unwrap();
      Arc::clone(&outer_lock.entry(dest_file_path.clone()).or_default())
    };
    let _guard = inner_lock_ref.write().unwrap();

    fs::rename(&tmp_file_path, &dest_file_path)?;
    let dir_file = fs::OpenOptions::new().read(true).open(&parent_directory)?;
    dir_file.sync_all()?;
    Ok(())
  };

  self.garbage_collect_locks();

  res
}
```

#### Ex 9: Connect to Peers
```rust
// Define the networking function
pub async fn start_network_listener(
    peer_manager: Arc<MockPeerManager>,
    listening_port: u16,
    stop_listen: Arc<AtomicBool>,
) {
    tokio::spawn(async move {
        let listener = tokio::net::TcpListener::bind(format!("[::]:{}", listening_port))
            .await
            .expect("Failed to bind to listen port - is something else already listening on it?");
        loop {
            let peer_mgr = peer_manager.clone();
            let tcp_stream = listener.accept().await.unwrap().0;
            if stop_listen.load(Ordering::Acquire) {
                return;
            }
            tokio::spawn(async move {
                setup_inbound(
                    peer_mgr.clone(),
                    tcp_stream.into_std().unwrap(),
                )
                .await;
            });
        }
    });
}
```

#### Ex 10: Handle Events
```rust
pub async fn handle_ldk_events(
    channel_manager: &ChannelManager, 
    bitcoind_client: BitcoindClient,
    keys_manager: KeysManager, 
    peer_manager: PeerManager,
    file_store: FileStore,
    event: Event
) {
    match event {
        Event::FundingGenerationReady {
            temporary_channel_id,
            counterparty_node_id,
            channel_value_satoshis,
            output_script,
            ..
        } => {
            // Construct the raw transaction with one output, that is paid the amount of the
            // channel.
            let addr = WitnessProgram::from_scriptpubkey(
                &output_script.as_bytes(),
                Regtest)
                .expect("Lightning funding tx should always be to a SegWit output")
                .to_address();

            let mut outputs = vec![HashMap::with_capacity(1)];
            outputs[0].insert(addr, channel_value_satoshis as f64 / 100_000_000.0);

            let raw_tx = bitcoind_client.create_raw_transaction(outputs).await;

            // Have your wallet put the inputs into the transaction such that the output is
            // satisfied.
            let funded_tx = bitcoind_client.fund_raw_transaction(raw_tx).await;

            // Sign the final funding transaction and give it to LDK, who will eventually broadcast it.
            let signed_tx = bitcoind_client.sign_raw_transaction_with_wallet(funded_tx).await;

            // Give the funding transaction back to LDK for opening the channel.
            channel_manager.funding_transaction_generated(
                temporary_channel_id,
                counterparty_node_id,
                signed_tx)
        },
        Event::FundingTxBroadcastSafe { .. } => {},
        Event::PaymentClaimable { .. } => {},
        Event::PendingHTLCsForwardable { .. } => {},
        Event::SpendableOutputs { .. } => {},
        Event::ChannelReady { .. } => {},
        Event::ChannelClosed { .. } => {},
        _ => {},
    }
}
```

#### Ex 11: Open Channel
```rust
pub fn open_channel(
  peer_pubkey: PublicKey, channel_amt_sat: u64, announce_for_forwarding: bool,
  with_anchors: bool, channel_manager: &ChannelManager,
) -> Result<(), ()> {

  let config = UserConfig {
    channel_handshake_limits: ChannelHandshakeLimits {
      // lnd's max to_self_delay is 2016, so we want to be compatible.
      their_to_self_delay: 2016,
      ..Default::default()
    },
    channel_handshake_config: ChannelHandshakeConfig {
      announce_for_forwarding,
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
```

#### Ex 12: Send Payment
```rust
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
```