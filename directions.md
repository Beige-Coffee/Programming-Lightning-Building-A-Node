## Start Bitcoind
```
source ./start.sh
```

## Start LDK

```
cargo run -- bitcoind:bitcoind@0.0.0.0:18443 ./ regtest
```


## Get Info for Core Lightning Node

```
l1-cli getinfo

```

...result
```
{
   "id": "037116dda737d37f019c0aa7404d592f31cd005ffa39dba338dd68ed280e843e4b",
   "alias": "YELLOWMONTANA",
   "color": "037116",
   "num_peers": 0,
   "num_pending_channels": 0,
   "num_active_channels": 0,
   "num_inactive_channels": 0,
   "address": [],
   "binding": [
      {
         "type": "ipv4",
         "address": "127.0.0.1",
         "port": 7070
      }
   ],
   "version": "v0.12.1",
   "blockheight": 151,
   "network": "regtest",
   "msatoshi_fees_collected": 0,
   "fees_collected_msat": "0msat",
   "lightning-dir": "/home/runner/workspace/.lightning/node1/regtest",
   "our_features": {
      "init": "08a000080269a2",
      "node": "88a000080269a2",
      "channel": "",
      "invoice": "02000000024100"
   }
}
```

## Connect to Peer Via LDK

```
connectpeer 037116dda737d37f019c0aa7404d592f31cd005ffa39dba338dd68ed280e843e4b@127.0.0.1:7070
```

## Open Channel
```
openchannel 037116dda737d37f019c0aa7404d592f31cd005ffa39dba338dd68ed280e843e4b@127.0.0.1:7070 100000
```

## Mine blocks

```
mine 6
```

... you should see that your channel is ready to use

```
EVENT: Channel 4d782cdb2e589427daa801d425aa19890d224ffcd3a98d0d9b5b24dac1403f2a with peer 037116dda737d37f019c0aa7404d592f31cd005ffa39dba338dd68ed280e843e4b is ready to be used!
```

## Get Invoice from Core Lightning

```
l1-cli invoice 1000sat 'inv-0' 'my first payment!'
```

## Send Payment With LDK

```
sendpayment lnbcrt10u1pnak2dxsp5k6xe3v5yykdlj0gx58fk3m9unspcdnkklj8sqhkmz9xg8vc30nnspp52359cca992w5k0yvet2gadfvk4r7y357dvexwprk0ekjjyvnd02qdqud4ujqenfwfehggrsv9uk6etwwsssxqyjw5qcqp29qyysgqkf2quh48uullpey7fvjne8p287umc8mz807pa2f562s35cj2782r2w8c907wvl82fpcfmdulvkfft2y9d4p2z3asrgasqdf40wp3djsq0x6qql
```