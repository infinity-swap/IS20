use candid::Principal;
use ic_helpers::tokens::Tokens128;

use crate::canister::erc20_transactions::{charge_fee, transfer_balance};
use crate::principal::{CheckedPrincipal, WithRecipient};
use crate::state::CanisterState;
use crate::types::{TxError, TxId, TxReceipt};

use super::TokenCanisterAPI;

/// Transfers `value` amount to the `to` principal, applying American style fee. This means, that
/// the recipient will receive `value - fee`, and the sender account will be reduced exactly by `value`.
///
/// Note, that the `value` cannot be less than the `fee` amount. If the value given is too small,
/// transaction will fail with `TxError::AmountTooSmall` error.
pub fn transfer_include_fee(
    canister: &impl TokenCanisterAPI,
    caller: CheckedPrincipal<WithRecipient>,
    amount: Tokens128,
) -> TxReceipt {
    let state = canister.state();
    let mut state = state.borrow_mut();
    let CanisterState {
        ref mut balances,
        ref mut ledger,
        // ref bidding_state,
        ref stats,
        ..
    } = *state;

    let (fee, fee_to) = stats.fee_info();
    let fee_ratio = canister.auction_state().borrow().bidding_state.fee_ratio;

    if amount <= fee {
        return Err(TxError::AmountTooSmall);
    }

    if balances.balance_of(&caller.inner()) < amount {
        return Err(TxError::InsufficientBalance);
    }

    charge_fee(balances, caller.inner(), fee_to, fee, fee_ratio)
        .expect("never fails due to checks above");
    transfer_balance(
        balances,
        caller.inner(),
        caller.recipient(),
        (amount - fee).expect("amount > fee is checked above"),
    )
    .expect("never fails due to checks above");

    let id = ledger.transfer(caller.inner(), caller.recipient(), amount, fee);
    Ok(id)
}

pub fn batch_transfer(
    canister: &impl TokenCanisterAPI,
    transfers: Vec<(Principal, Tokens128)>,
) -> Result<Vec<TxId>, TxError> {
    let from = ic_canister::ic_kit::ic::caller();
    let state = canister.state();
    let mut state = state.borrow_mut();

    let mut total_value = Tokens128::from(0u128);
    for target in transfers.iter() {
        total_value = (total_value + target.1).ok_or(TxError::AmountOverflow)?;
    }

    let CanisterState {
        ref mut balances,
        ref stats,
        ..
    } = &mut *state;

    let auction_state = canister.auction_state();
    let bidding_state = &mut auction_state.borrow_mut().bidding_state;
    let (fee, fee_to) = stats.fee_info();
    let fee_ratio = bidding_state.fee_ratio;

    let total_fee = (fee * transfers.len())
        .to_tokens128()
        .ok_or(TxError::AmountOverflow)?;

    if balances.balance_of(&from) < (total_value + total_fee).ok_or(TxError::AmountOverflow)? {
        return Err(TxError::InsufficientBalance);
    }

    {
        for (to, value) in transfers.clone() {
            charge_fee(balances, from, fee_to, fee, fee_ratio)
                .expect("never fails due to checks above");
            transfer_balance(balances, from, to, value).expect("never fails due to checks above");
        }
    }

    let id = state.ledger.batch_transfer(from, transfers, fee);
    Ok(id)
}

#[cfg(test)]
mod tests {
    use ic_canister::{
        ic_kit::{
            mock_principals::{alice, bob, john, xtc},
            MockContext,
        },
        Canister,
    };

    use crate::mock::*;
    use crate::types::Metadata;

    use super::*;

    fn test_canister() -> TokenCanisterMock {
        MockContext::new().with_caller(alice()).inject();

        let canister = TokenCanisterMock::init_instance();
        canister.init(Metadata {
            logo: "".to_string(),
            name: "".to_string(),
            symbol: "".to_string(),
            decimals: 8,
            totalSupply: Tokens128::from(1000),
            owner: alice(),
            fee: Tokens128::from(0),
            feeTo: alice(),
            isTestToken: None,
        });

        // This is to make tests that don't rely on auction state
        // pass, because since we are running auction state on each
        // endpoint call, it affects `BiddingInfo.fee_ratio` that is
        // used for charging fees in `approve` endpoint.
        canister.state.borrow_mut().stats.min_cycles = 0;

        canister
    }

    #[test]
    fn batch_transfer_without_fee() {
        let canister = test_canister();
        assert_eq!(Tokens128::from(1000), canister.balanceOf(alice()));
        let transfers = vec![
            (bob(), Tokens128::from(100)),
            (john(), Tokens128::from(200)),
        ];
        let receipt = canister.batchTransfer(transfers).unwrap();
        assert_eq!(receipt.len(), 2);
        assert_eq!(canister.balanceOf(alice()), Tokens128::from(700));
        assert_eq!(canister.balanceOf(bob()), Tokens128::from(100));
        assert_eq!(canister.balanceOf(john()), Tokens128::from(200));
    }

    #[test]
    fn batch_transfer_with_fee() {
        let canister = test_canister();
        let state = canister.state();
        let mut state = state.borrow_mut();
        state.stats.fee = Tokens128::from(50);
        state.stats.fee_to = john();
        drop(state);
        assert_eq!(Tokens128::from(1000), canister.balanceOf(alice()));
        let transfers = vec![(bob(), Tokens128::from(100)), (xtc(), Tokens128::from(200))];
        let receipt = canister.batchTransfer(transfers).unwrap();
        assert_eq!(receipt.len(), 2);
        assert_eq!(canister.balanceOf(alice()), Tokens128::from(600));
        assert_eq!(canister.balanceOf(bob()), Tokens128::from(100));
        assert_eq!(canister.balanceOf(xtc()), Tokens128::from(200));
        assert_eq!(canister.balanceOf(john()), Tokens128::from(100));
    }

    #[test]
    fn batch_transfer_insufficient_balance() {
        let canister = test_canister();
        let transfers = vec![
            (bob(), Tokens128::from(500)),
            (john(), Tokens128::from(600)),
        ];
        let receipt = canister.batchTransfer(transfers);
        assert!(receipt.is_err());
        assert_eq!(receipt.unwrap_err(), TxError::InsufficientBalance);
        assert_eq!(canister.balanceOf(alice()), Tokens128::from(1000));
        assert_eq!(canister.balanceOf(bob()), Tokens128::from(0));
        assert_eq!(canister.balanceOf(john()), Tokens128::from(0));
    }

    #[test]
    fn transfer_without_fee() {
        let canister = test_canister();
        assert_eq!(Tokens128::from(1000), canister.balanceOf(alice()));

        assert!(canister
            .transferIncludeFee(bob(), Tokens128::from(100))
            .is_ok());
        assert_eq!(canister.balanceOf(bob()), Tokens128::from(100));
        assert_eq!(canister.balanceOf(alice()), Tokens128::from(900));
    }

    #[test]
    fn transfer_with_fee() {
        let canister = test_canister();

        let state = canister.state();
        let mut state = state.borrow_mut();
        state.stats.fee = Tokens128::from(100);
        state.stats.fee_to = john();
        drop(state);

        assert!(canister
            .transferIncludeFee(bob(), Tokens128::from(200))
            .is_ok());
        assert_eq!(canister.balanceOf(bob()), Tokens128::from(100));
        assert_eq!(canister.balanceOf(alice()), Tokens128::from(800));
        assert_eq!(canister.balanceOf(john()), Tokens128::from(100));
    }

    #[test]
    fn transfer_insufficient_balance() {
        let canister = test_canister();
        assert_eq!(
            canister.transferIncludeFee(bob(), Tokens128::from(1001)),
            Err(TxError::InsufficientBalance)
        );
        assert_eq!(canister.balanceOf(alice()), Tokens128::from(1000));
        assert_eq!(canister.balanceOf(bob()), Tokens128::from(0));
    }
}
