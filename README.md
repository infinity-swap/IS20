![IS20 banner](https://user-images.githubusercontent.com/6412426/146728389-42384977-0ed3-43a6-83d3-ce16db609c09.png)

# IS20 - Introduction

IS20 is an Internet Computer token standard proposed by Infinity Swap.

You can find the standard spec at [spec/IS20.md](spec/IS20.md) and the default implementation in the `src` directory.

This repository contains two canisters:
* `factory` is responsible for creating and deploying  new token canisters
* `token` is the default implementation of the IS20 token

# Usage

You can try using the factory and tokens using `dfx` tool. To do so, install and start `dfx`:

```shell
sh -ci "$(curl -fsSL https://sdk.dfinity.org/install.sh)"

dfx start --background
```

To build the canister you will also need the `ic_cdk_optimizer` tool:

```
cargo install ic-cdk-optimizer
```

Then deploy the factory:

```shell
dfx identity get-principal
>> y4nw3-upugh-yyv2b-jv6jy-ppfse-4fkfd-uaqv5-woqup-u3cx3-hah2c-yae

// Use the user principal above to set the owner
dfx deploy token_factory --argument '(principal "y4nw3-upugh-yyv2b-jv6jy-ppfse-4fkfd-uaqv5-woqup-u3cx3-hah2c-yae", null)'

>> Creating a wallet canister on the local network.
>> The wallet canister on the "local" network for user "max" is "yjeau-xiaaa-aaaaa-aabsa-cai"
>> Deploying: token_factory
>> Creating canisters...
>> Creating canister "token_factory"...
>> "token_factory" canister created with canister id: "yofga-2qaaa-aaaaa-aabsq-cai"

```

Note the wallet ID for the current user (in the example above it's `yjeau-xiaaa-aaaaa-aabsa-cai`). The factory requires
the caller to provide cycles or ICP to create a token canister. As we don't have an ICP ledger locally, we use cycles.
The minimum amount of cycles required by the factory to create a canister is `10^12`.

```shell
// Use the user principal above to set the owner
dfx canister --wallet yjeau-xiaaa-aaaaa-aabsa-cai call --with-cycles 1000000000000 token_factory create_token \
  '(record {
  logo = "";
  name = "y";
  symbol = "y";
  decimals = 8;
  total_supply = 1000000000;
  owner = principal "y4nw3-upugh-yyv2b-jv6jy-ppfse-4fkfd-uaqv5-woqup-u3cx3-hah2c-yae";
  fee = 0;
  feeTo = principal "y4nw3-upugh-yyv2b-jv6jy-ppfse-4fkfd-uaqv5-woqup-u3cx3-hah2c-yae"; }, null)'

>> (variant { principal "r7inp-6aaaa-aaaaa-aaabq-cai" })
```

The returned principal id is the token canister principal. You can use this id to make token calls:

```shell
// Tokens transfer
dfx canister call r7inp-6aaaa-aaaaa-aaabq-cai transfer '(principal "aaaaa-aa", 1000: nat)'
>> (variant { 17_724 = 2 : nat })

// Get transaction information
dfx canister call r7inp-6aaaa-aaaaa-aaabq-cai get_transaction '(1:nat)'
>> (
>>   record {
>>     25_979 = principal "aaaaa-aa";
>>     5_094_982 = 0 : nat;
>>     100_394_802 = variant { 2_633_774_657 };
>>     1_136_829_802 = principal "y4nw3-upugh-yyv2b-jv6jy-ppfse-4fkfd-uaqv5-woqup-u3cx3-hah2c-yae";
>>     2_688_582_695 = variant { 3_021_957_963 };
>>     2_781_795_542 = 1_640_332_539_774_695_111 : int;
>>     3_068_679_307 = opt principal "y4nw3-upugh-yyv2b-jv6jy-ppfse-4fkfd-uaqv5-woqup-u3cx3-hah2c-yae";
>>     3_189_021_458 = 2 : nat;
>>     3_573_748_184 = 1_000 : nat;
>>   },
>> )
```

To bid cycles for the cycle auction, you need to provide the cycles with your call. Use cycle wallet
to do so:

```shell
dfx identity get-wallet
>> rwlgt-iiaaa-aaaaa-aaaaa-cai

dfx canister --wallet rwlgt-iiaaa-aaaaa-aaaaa-cai call --with-cycles 100000000 \
  r7inp-6aaaa-aaaaa-aaabq-cai bidCycles \
  '(principal "y4nw3-upugh-yyv2b-jv6jy-ppfse-4fkfd-uaqv5-woqup-u3cx3-hah2c-yae")'
>> (variant { 17_724 = 100_000_000 : nat64 })

```

# Development

## Building

Use build script to build the release version of the token canister, use the build script:

```shell
./scripts/build.sh
```

## Running tests

In order to run tests:

```shell
cargo test
```

## Enable pre-commit

Before committing to this repo, install and activate the `pre-commit` tool.

```shell
pip install pre-commit
pre-commit install
```

## Local Run

```bash
dfx start --background
dfx deploy
dfx stop
```

## Candid Files

In order to generate candid files, run the following command:

```bash
cargo run -p factory > src/candid/token-factory.did
cargo run -p token > src/candid/token.did
```
