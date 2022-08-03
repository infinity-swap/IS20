# IS20 changelog

There has been substantial changes to the IS20 token standard.
We have adapted the ICRC-1 token methods while retaining the IS20 standard with minimal changes.

## Changes

The following API changes have been made to the IS20 token standard :-

### Transfer

- The `transfer` function has been renamed into `icrc_transfer`. It takes seven arguments namely `from_subaccount`, `to`, `to_subaccount`, `amount`, `fee`, `memo` and `created_at_time`.
- The `transferIncludeFee` has renamed to  `icrc_transferIncludeFee`. It takes one less argument namely `fee` from the `icrc1_transfer` function.

### Burn

- The `burn` function has been renamed into: `icrc1_burn`.
- We are supporting from `subaccount` by  specifiying the subaccount argument. When specified the burn will be done from the subaccount.

### Mint

- The `mint` function has been rename into  `icrc1_mint`.
- The `mint` includes support for the `subaccount` argument, which is used to specify the subaccount to mint the token to.

### The following minimal api changes have been made to the IS20 token standard :-

- `getTransactions` now gets the transactions of principal and subaccounts.
- `name` function has been renamed to `icrc1_name`.
- `symbol` function has been renamed to `icrc1_symbol`.
- `decimals` function has been renamed to `icrc1_decimals`.
- `icrc1_metadata` has been added to the token standard, this retrieves the metadata of the token in this format
  `(vec record { name : text; url : text })`

### Deprecated

- `transferFrom`
- `approve`
- `notify` methods
