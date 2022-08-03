use ic_helpers::tokens::Tokens128;

use crate::account::{Account, Subaccount};
use crate::canister::erc20_transactions::{charge_fee, transfer_balance};
use crate::state::CanisterState;
use crate::types::{BatchTransferArgs, Timestamp, TxError, TxId, TxReceipt};

use super::erc20_transactions::ONE_MIN_IN_NANOS;
use super::TokenCanisterAPI;

/// Transfers `value` amount to the `to` principal, applying American style fee. This means, that
/// the recipient will receive `value - fee`, and the sender account will be reduced exactly by `value`.
///
/// Note, that the `value` cannot be less than the `fee` amount. If the value given is too small,
/// transaction will fail with `TxError::AmountTooSmall` error.
pub fn icrc1_transfer_include_fee(
    canister: &impl TokenCanisterAPI,
    from: Account,
    to: Account,
    amount: Tokens128,
    memo: Option<u64>,
    created_at_time: Option<Timestamp>,
) -> TxReceipt {
    let state = canister.state();
    let mut state = state.borrow_mut();
    let CanisterState {
        ref mut balances,
        ref mut ledger,
        ref bidding_state,
        ref stats,
        ..
    } = *state;

    let now = ic_canister::ic_kit::ic::time();

    // We check if the `created_at_time` is within the ONE MINUTE WINDOW TIME,
    // if it is less than or greater than ONE MINUTE WINDOW, we reject the
    // transaction.
    let created_at_time = match created_at_time {
        Some(created_at_time) => {
            if now.abs_diff(created_at_time) > ONE_MIN_IN_NANOS {
                return Err(TxError::GenericError {
                    message: "Created time is too far in the past or future".to_string(),
                });
            }
            created_at_time
        }

        None => now,
    };

    let (fee, fee_to) = stats.fee_info();
    let fee_ratio = bidding_state.fee_ratio;

    if amount <= fee {
        return Err(TxError::AmountTooSmall);
    }

    let balance = balances.balance_of(from);

    if balance < amount {
        return Err(TxError::InsufficientFunds { balance });
    }

    charge_fee(balances, from, fee_to, fee, fee_ratio).expect("never fails due to checks above");
    transfer_balance(
        balances,
        from,
        to,
        (amount - fee).expect("amount > fee is checked above"),
    )
    .expect("never fails due to checks above");

    let id = ledger.transfer(from, to, amount, fee, memo, created_at_time);
    Ok(id.into())
}

pub fn batch_transfer(
    canister: &impl TokenCanisterAPI,
    from_subaccount: Option<Subaccount>,
    transfers: Vec<BatchTransferArgs>,
) -> Result<Vec<TxId>, TxError> {
    let caller = ic_canister::ic_kit::ic::caller();
    let from = Account::new(caller, from_subaccount);
    let state = canister.state();
    let mut state = state.borrow_mut();

    let mut total_value = Tokens128::from(0u128);
    for target in &transfers {
        total_value = (total_value + target.amount).ok_or(TxError::AmountOverflow)?;
    }

    let CanisterState {
        ref mut balances,
        ref bidding_state,
        ref mut ledger,
        ref stats,
        ..
    } = &mut *state;

    let (fee, fee_to) = stats.fee_info();
    let fee_ratio = bidding_state.fee_ratio;

    let total_fee = (fee * transfers.len())
        .to_tokens128()
        .ok_or(TxError::AmountOverflow)?;

    let balance = balances.balance_of(from);

    if balance < (total_value + total_fee).ok_or(TxError::AmountOverflow)? {
        return Err(TxError::InsufficientFunds { balance });
    }

    {
        for x in &transfers {
            let value = x.amount;
            let to = Account::new(x.receiver.principal, x.receiver.subaccount);
            charge_fee(balances, from, fee_to, fee, fee_ratio)
                .expect("never fails due to checks above");
            transfer_balance(balances, from, to, value).expect("never fails due to checks above");
        }
    }

    let id = ledger.batch_transfer(from, transfers, fee);
    Ok(id)
}

#[cfg(test)]
mod tests {
    use ic_canister::ic_kit::mock_principals::{alice, bob, john, xtc};
    use ic_canister::ic_kit::MockContext;
    use ic_canister::Canister;
    use rand::{thread_rng, Rng};

    use crate::mock::TokenCanisterMock;
    use crate::types::Metadata;

    use super::*;

    // Method for generating random Subaccount.
    fn gen_subaccount() -> Subaccount {
        // generate a random subaccount
        let mut subaccount = [0u8; 32];
        thread_rng().fill(&mut subaccount);
        subaccount
    }

    fn test_canister() -> TokenCanisterMock {
        MockContext::new().with_caller(alice()).inject();

        let canister = TokenCanisterMock::init_instance();
        canister.init(
            Metadata {
                logo: "".to_string(),
                name: "".to_string(),
                symbol: "".to_string(),
                decimals: 8,
                owner: alice(),
                fee: Tokens128::from(0),
                feeTo: alice(),
                isTestToken: None,
            },
            Tokens128::from(1000),
        );

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
        assert_eq!(
            Tokens128::from(1000),
            canister.icrc1_balance_of((alice(), None).into())
        );
        let transfer1 = BatchTransferArgs {
            receiver: Account {
                principal: bob(),
                subaccount: None,
            },
            amount: Tokens128::from(100),
        };
        let transfer2 = BatchTransferArgs {
            receiver: Account {
                principal: john(),
                subaccount: None,
            },
            amount: Tokens128::from(200),
        };
        let receipt = canister
            .batchTransfer(None, vec![transfer1, transfer2])
            .unwrap();
        assert_eq!(receipt.len(), 2);
        assert_eq!(
            canister.icrc1_balance_of((alice(), None).into()),
            Tokens128::from(700)
        );
        assert_eq!(
            canister.icrc1_balance_of((bob(), None).into()),
            Tokens128::from(100)
        );
        assert_eq!(
            canister.icrc1_balance_of((john(), None).into()),
            Tokens128::from(200)
        );
    }

    #[test]
    fn batch_transfer_with_fee() {
        let canister = test_canister();
        let mut state = canister.state.borrow_mut();
        state.stats.fee = Tokens128::from(50);
        state.stats.fee_to = john();
        drop(state);
        assert_eq!(
            Tokens128::from(1000),
            canister.icrc1_balance_of((alice(), None).into())
        );
        let transfer1 = BatchTransferArgs {
            receiver: Account {
                principal: bob(),
                subaccount: None,
            },
            amount: Tokens128::from(100),
        };
        let transfer2 = BatchTransferArgs {
            receiver: Account {
                principal: xtc(),
                subaccount: None,
            },
            amount: Tokens128::from(200),
        };
        let receipt = canister
            .batchTransfer(None, vec![transfer1, transfer2])
            .unwrap();
        assert_eq!(receipt.len(), 2);
        assert_eq!(
            canister.icrc1_balance_of((alice(), None).into()),
            Tokens128::from(600)
        );
        assert_eq!(
            canister.icrc1_balance_of((bob(), None).into()),
            Tokens128::from(100)
        );
        assert_eq!(
            canister.icrc1_balance_of((xtc(), None).into()),
            Tokens128::from(200)
        );
        assert_eq!(
            canister.icrc1_balance_of((john(), None).into()),
            Tokens128::from(100)
        );
    }

    #[test]
    fn batch_transfer_insufficient_balance() {
        let canister = test_canister();

        let transfer1 = BatchTransferArgs {
            receiver: Account {
                principal: bob(),
                subaccount: None,
            },
            amount: Tokens128::from(500),
        };
        let transfer2 = BatchTransferArgs {
            receiver: Account {
                principal: john(),
                subaccount: None,
            },
            amount: Tokens128::from(600),
        };
        let receipt = canister.batchTransfer(None, vec![transfer1, transfer2]);
        assert!(receipt.is_err());
        let balance = canister.icrc1_balance_of((alice(), None).into());
        assert_eq!(receipt.unwrap_err(), TxError::InsufficientFunds { balance });
        assert_eq!(
            canister.icrc1_balance_of((alice(), None).into()),
            Tokens128::from(1000)
        );
        assert_eq!(
            canister.icrc1_balance_of((bob(), None).into()),
            Tokens128::from(0)
        );
        assert_eq!(
            canister.icrc1_balance_of((john(), None).into()),
            Tokens128::from(0)
        );
    }

    #[test]
    fn transfer_without_fee() {
        let canister = test_canister();
        let bob_sub = gen_subaccount();
        assert_eq!(
            Tokens128::from(1000),
            canister.icrc1_balance_of((alice(), None).into())
        );

        assert!(canister
            .icrc1_transferIncludeFee(None, bob(), None, Tokens128::from(100), None, None)
            .is_ok());
        assert_eq!(
            canister.icrc1_balance_of((bob(), None).into()),
            Tokens128::from(100)
        );
        assert_eq!(
            canister.icrc1_balance_of((alice(), None).into()),
            Tokens128::from(900)
        );

        assert!(canister
            .icrc1_transferIncludeFee(None, bob(), Some(bob_sub), Tokens128::from(100), None, None)
            .is_ok());
        assert_eq!(
            canister.icrc1_balance_of((bob(), Some(bob_sub)).into()),
            Tokens128::from(100)
        );
    }

    #[test]
    fn transfer_with_fee() {
        let bob_sub = gen_subaccount();
        let canister = test_canister();
        let mut state = canister.state.borrow_mut();
        state.stats.fee = Tokens128::from(100);
        state.stats.fee_to = john();
        drop(state);

        assert!(canister
            .icrc1_transferIncludeFee(None, bob(), None, Tokens128::from(200), None, None)
            .is_ok());
        assert_eq!(
            canister.icrc1_balance_of((bob(), None).into()),
            Tokens128::from(100)
        );
        assert_eq!(
            canister.icrc1_balance_of((alice(), None).into()),
            Tokens128::from(800)
        );
        assert_eq!(
            canister.icrc1_balance_of((john(), None).into()),
            Tokens128::from(100)
        );

        assert!(canister
            .icrc1_transferIncludeFee(None, bob(), Some(bob_sub), Tokens128::from(150), None, None)
            .is_ok());
        assert_eq!(
            canister.icrc1_balance_of((bob(), Some(bob_sub)).into()),
            Tokens128::from(50)
        );
    }

    #[test]
    fn transfer_insufficient_balance() {
        let canister = test_canister();
        let balance = canister.icrc1_balance_of((alice(), None).into());
        assert!(canister
            .icrc1_transferIncludeFee(None, bob(), None, Tokens128::from(1001), None, None)
            .is_err());
        assert_eq!(
            canister.icrc1_transferIncludeFee(None, bob(), None, Tokens128::from(1001), None, None),
            Err(TxError::InsufficientFunds { balance })
        );
        assert_eq!(
            canister.icrc1_balance_of((alice(), None).into()),
            Tokens128::from(1000)
        );
        assert_eq!(
            canister.icrc1_balance_of((bob(), None).into()),
            Tokens128::from(0)
        );
    }
}
