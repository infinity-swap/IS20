use ic_canister::ic_kit::ic;
use ic_cdk::export::Principal;
use ic_helpers::ledger::AccountIdentifier;
use ic_helpers::ledger::Subaccount as SubaccountIdentifier;
use ic_helpers::tokens::Tokens128;

use crate::account::{Account, CheckedAccount, Subaccount, WithRecipient};
use crate::canister::is20_auction::auction_principal;
use crate::principal::{CheckedPrincipal, Owner, TestNet};
use crate::state::{Balances, CanisterState};
use crate::types::{TransferArgs, TxError, TxReceipt};

use super::TokenCanisterAPI;

pub static ONE_MIN_IN_NANOS: u64 = 60_000_000_000;

pub(crate) fn icrc1_transfer(
    canister: &impl TokenCanisterAPI,
    caller: CheckedAccount<WithRecipient>,
    transfer: TransferArgs,
) -> TxReceipt {
    let TransferArgs {
        amount,
        memo,
        created_at_time,
        ..
    } = transfer;

    let now = ic::time();

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

    let from = caller.inner();
    let to = caller.recipient();

    let state = canister.state();
    let mut state = state.borrow_mut();
    let CanisterState {
        ref mut balances,
        ref bidding_state,
        ref stats,
        ..
    } = &mut *state;

    let (fee, fee_to) = stats.fee_info();
    let fee_ratio = bidding_state.fee_ratio;

    if let Some(fee_limit) = transfer.fee {
        if fee > fee_limit {
            return Err(TxError::FeeExceededLimit);
        }
    }

    let balance = balances.balance_of(from);

    if balance < (amount + fee).ok_or(TxError::AmountOverflow)? {
        return Err(TxError::InsufficientFunds { balance });
    }

    charge_fee(balances, from, fee_to, fee, fee_ratio).expect("never fails due to checks above");

    transfer_balance(balances, from, to, amount).expect("never fails due to checks above");

    let id = state
        .ledger
        .transfer(from, to, amount, fee, memo, created_at_time);
    Ok(id.into())
}

fn mint(state: &mut CanisterState, caller: Principal, to: Account, amount: Tokens128) -> TxReceipt {
    let balance = state.balances.get_mut_or_insert_default(to);

    let new_balance = (*balance + amount)
        .expect("balance cannot be larger than total_supply which is already checked");

    *balance = new_balance;

    let id = state.ledger.mint(caller.into(), to, amount);

    Ok(id.into())
}

pub fn mint_test_token(
    state: &mut CanisterState,
    caller: CheckedPrincipal<TestNet>,
    to: Principal,
    to_subaccount: Option<Subaccount>,
    amount: Tokens128,
) -> TxReceipt {
    mint(
        state,
        caller.inner(),
        Account::new(to, to_subaccount),
        amount,
    )
}

pub fn mint_as_owner(
    state: &mut CanisterState,
    caller: CheckedPrincipal<Owner>,
    to: Principal,
    to_subaccount: Option<Subaccount>,
    amount: Tokens128,
) -> TxReceipt {
    mint(
        state,
        caller.inner(),
        Account::new(to, to_subaccount),
        amount,
    )
}

pub fn burn(
    state: &mut CanisterState,
    caller: Principal,
    from: Account,
    amount: Tokens128,
) -> TxReceipt {
    let balance = state.balances.balance_of(from);

    if !amount.is_zero() && balance == Tokens128::ZERO {
        return Err(TxError::InsufficientFunds { balance });
    }

    let new_balance = (balance - amount).ok_or(TxError::InsufficientFunds { balance })?;

    if new_balance == Tokens128::ZERO {
        state.balances.remove(from)
    } else {
        state.balances.set_balance(from, new_balance)
    }

    let id = state.ledger.burn(caller.into(), from, amount);
    Ok(id.into())
}

pub fn burn_own_tokens(
    state: &mut CanisterState,
    from_subaccount: Option<Subaccount>,
    amount: Tokens128,
) -> TxReceipt {
    let caller = ic::caller();
    burn(state, caller, Account::new(caller, from_subaccount), amount)
}

pub fn burn_as_owner(
    state: &mut CanisterState,
    caller: CheckedPrincipal<Owner>,
    from: Principal,
    from_subaccount: Option<Subaccount>,
    amount: Tokens128,
) -> TxReceipt {
    burn(
        state,
        caller.inner(),
        Account::new(from, from_subaccount),
        amount,
    )
}

pub fn mint_to_accountid(
    state: &mut CanisterState,
    to: AccountIdentifier,
    amount: Tokens128,
) -> Result<(), TxError> {
    let balance = state.claims.entry(to).or_default();
    let new_balance = (*balance + amount)
        .expect("balance cannot be larger than total_supply which is already checked");
    *balance = new_balance;
    Ok(())
}

pub fn claim(
    state: &mut CanisterState,
    account: AccountIdentifier,
    subaccount: Option<Subaccount>,
) -> TxReceipt {
    let caller = ic_canister::ic_kit::ic::caller();
    let amount = state.claim_amount(account);

    if account
        != AccountIdentifier::new(
            caller.into(),
            Some(SubaccountIdentifier(subaccount.unwrap_or_default())),
        )
    {
        return Err(TxError::ClaimNotAllowed);
    }
    let to = Account::new(caller, subaccount);

    let id = mint(state, caller, to, amount);

    state.claims.remove(&account);

    id
}

pub fn transfer_balance(
    balances: &mut Balances,
    from: Account,
    to: Account,
    amount: Tokens128,
) -> Result<(), TxError> {
    if amount == Tokens128::ZERO {
        return Ok(());
    }

    let from_balance = balances.get_mut(from).ok_or(TxError::InsufficientFunds {
        balance: Tokens128::ZERO,
    })?;

    *from_balance = (*from_balance - amount).ok_or(TxError::InsufficientFunds {
        balance: *from_balance,
    })?;

    let to_balance = balances.get_mut_or_insert_default(to);

    *to_balance = (*to_balance + amount).expect(
        "never overflows since `from_balance + to_balance` is limited by `total_supply` amount",
    );

    if balances.balance_of(from) == Tokens128::ZERO {
        balances.remove(from);
    }

    Ok(())
}

pub(crate) fn charge_fee(
    balances: &mut Balances,
    user: Account,
    fee_to: Principal,
    fee: Tokens128,
    fee_ratio: f64,
) -> Result<(), TxError> {
    // todo: check if this is enforced
    debug_assert!((0.0..=1.0).contains(&fee_ratio));

    if fee == Tokens128::ZERO {
        return Ok(());
    }

    // todo: test and figure out overflows
    const INT_CONVERSION_K: u128 = 1_000_000_000_000;
    let auction_fee_amount = (fee * Tokens128::from((fee_ratio * INT_CONVERSION_K as f64) as u128)
        / INT_CONVERSION_K)
        .expect("never division by 0");
    let auction_fee_amount = auction_fee_amount
        .to_tokens128()
        .expect("fee is always greater");
    let owner_fee_amount = (fee - auction_fee_amount).expect("fee is always greater");
    transfer_balance(balances, user, fee_to.into(), owner_fee_amount)?;
    transfer_balance(
        balances,
        user,
        auction_principal().into(),
        auction_fee_amount,
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::UNIX_EPOCH;

    use ic_canister::ic_kit::mock_principals::{alice, bob, john, xtc};
    use ic_canister::ic_kit::MockContext;
    use ic_canister::Canister;
    use rand::prelude::*;

    use crate::mock::*;
    use crate::types::{Metadata, Operation, TransactionStatus};

    use super::*;

    // Method for generating random Subaccount.
    fn gen_subaccount() -> Subaccount {
        let mut subaccount = [0u8; 32];
        thread_rng().fill(&mut subaccount);
        subaccount
    }

    fn test_context() -> (&'static MockContext, TokenCanisterMock) {
        let context = MockContext::new().with_caller(alice()).inject();

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

        (context, canister)
    }

    fn test_canister() -> TokenCanisterMock {
        let (_, canister) = test_context();
        canister
    }

    #[test]
    fn transfer_without_fee() {
        let canister = test_canister();
        let alice_sub = gen_subaccount();
        let bob_sub = gen_subaccount();

        assert_eq!(
            Tokens128::from(1000),
            canister.icrc1_balance_of((alice(), None).into())
        );

        let transfer1 = TransferArgs {
            from_subaccount: None,
            to: Account::from(bob()),
            amount: Tokens128::from(100),
            fee: None,
            memo: None,
            created_at_time: None,
        };

        assert!(canister.icrc1_transfer(transfer1).is_ok());
        assert_eq!(
            canister.icrc1_balance_of((bob(), None).into()),
            Tokens128::from(100)
        );
        assert_eq!(
            canister.icrc1_balance_of((alice(), None).into()),
            Tokens128::from(900)
        );

        assert!(canister
            .icrc1_mint(alice(), Some(alice_sub), Tokens128::from(100))
            .is_ok());

        let transfer2 = TransferArgs {
            from_subaccount: Some(alice_sub),
            to: Account::new(bob(), Some(bob_sub)),
            amount: Tokens128::from(50),
            fee: None,
            memo: None,
            created_at_time: None,
        };
        assert!(canister.icrc1_transfer(transfer2).is_ok());
        assert_eq!(
            canister.icrc1_balance_of((alice(), Some(alice_sub)).into()),
            Tokens128::from(50)
        );
        assert_eq!(
            canister.icrc1_balance_of((bob(), Some(bob_sub)).into()),
            Tokens128::from(50)
        );
        assert_eq!(canister.icrc1_total_supply(), Tokens128::from(1100));
    }

    #[test]
    fn transfer_with_fee() {
        let canister = test_canister();
        let alice_sub = gen_subaccount();
        let bob_sub = gen_subaccount();
        canister.state().borrow_mut().stats.fee = Tokens128::from(100);
        canister.state().borrow_mut().stats.fee_to = john();

        let transfer1 = TransferArgs {
            from_subaccount: None,
            to: Account::from(bob()),
            amount: Tokens128::from(200),
            fee: None,
            memo: None,
            created_at_time: None,
        };

        assert!(canister.icrc1_transfer(transfer1).is_ok());
        assert_eq!(
            canister.icrc1_balance_of((bob(), None).into()),
            Tokens128::from(200)
        );
        assert_eq!(
            canister.icrc1_balance_of((alice(), None).into()),
            Tokens128::from(700)
        );
        assert_eq!(
            canister.icrc1_balance_of((john(), None).into()),
            Tokens128::from(100)
        );

        assert!(canister
            .icrc1_mint(alice(), Some(alice_sub), Tokens128::from(1000))
            .is_ok());

        let transfer2 = TransferArgs {
            from_subaccount: Some(alice_sub),
            to: Account::new(bob(), Some(bob_sub)),

            amount: Tokens128::from(500),
            fee: None,
            memo: None,
            created_at_time: None,
        };
        assert!(canister.icrc1_transfer(transfer2).is_ok());

        assert_eq!(
            canister.icrc1_balance_of((bob(), Some(bob_sub)).into()),
            Tokens128::from(500)
        );
        assert_eq!(
            canister.icrc1_balance_of((alice(), Some(alice_sub)).into()),
            Tokens128::from(400)
        );
    }

    #[test]
    fn transfer_fee_exceeded() {
        let canister = test_canister();
        canister.state().borrow_mut().stats.fee = Tokens128::from(100);
        canister.state().borrow_mut().stats.fee_to = john();

        let transfer1 = TransferArgs {
            from_subaccount: None,
            to: Account::from(bob()),
            amount: Tokens128::from(200),
            fee: Some(Tokens128::from(100)),
            memo: None,
            created_at_time: None,
        };

        assert!(canister.icrc1_transfer(transfer1).is_ok());

        let transfer2 = TransferArgs {
            from_subaccount: None,
            to: Account::from(bob()),
            amount: Tokens128::from(200),
            fee: Some(Tokens128::from(50)),
            memo: None,
            created_at_time: None,
        };
        assert_eq!(
            canister.icrc1_transfer(transfer2),
            Err(TxError::FeeExceededLimit)
        );

        let transfer3 = TransferArgs {
            from_subaccount: None,
            to: Account::new(bob(), Some(gen_subaccount())),
            amount: Tokens128::from(200),
            fee: Some(Tokens128::from(50)),
            memo: None,
            created_at_time: None,
        };
        assert_eq!(
            canister.icrc1_transfer(transfer3),
            Err(TxError::FeeExceededLimit)
        );
    }

    #[test]
    fn fees_with_auction_enabled() {
        let canister = test_canister();
        canister.state().borrow_mut().stats.fee = Tokens128::from(50);
        canister.state().borrow_mut().stats.fee_to = john();
        canister.state().borrow_mut().stats.min_cycles = crate::types::DEFAULT_MIN_CYCLES;
        canister.state().borrow_mut().bidding_state.fee_ratio = 0.5;

        let transfer1 = TransferArgs {
            from_subaccount: None,
            to: Account::from(bob()),
            amount: Tokens128::from(100),
            fee: None,
            memo: None,
            created_at_time: None,
        };

        canister.icrc1_transfer(transfer1).unwrap();
        assert_eq!(
            canister.icrc1_balance_of((bob(), None).into()),
            Tokens128::from(100)
        );
        assert_eq!(
            canister.icrc1_balance_of((alice(), None).into()),
            Tokens128::from(850)
        );
        assert_eq!(
            canister.icrc1_balance_of((john(), None).into()),
            Tokens128::from(25)
        );
        assert_eq!(
            canister.icrc1_balance_of((auction_principal(), None).into()),
            Tokens128::from(25)
        );
    }

    #[test]
    fn transfer_insufficient_balance() {
        let canister = test_canister();

        let transfer1 = TransferArgs {
            from_subaccount: None,
            to: Account::from(bob()),
            amount: Tokens128::from(1001),
            fee: None,
            memo: None,
            created_at_time: None,
        };
        let balance = canister.icrc1_balance_of((alice(), None).into());
        assert_eq!(
            canister.icrc1_transfer(transfer1),
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

    #[test]
    fn transfer_with_fee_insufficient_balance() {
        let canister = test_canister();
        canister.state().borrow_mut().stats.fee = Tokens128::from(100);
        canister.state().borrow_mut().stats.fee_to = john();

        let transfer1 = TransferArgs {
            from_subaccount: None,
            to: Account::from(bob()),
            amount: Tokens128::from(950),
            fee: None,
            memo: None,
            created_at_time: None,
        };

        let balance = canister.icrc1_balance_of((alice(), None).into());

        assert_eq!(
            canister.icrc1_transfer(transfer1),
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

    #[test]
    fn transfer_wrong_caller() {
        let canister = test_canister();
        MockContext::new().with_caller(bob()).inject();
        let transfer1 = TransferArgs {
            from_subaccount: None,
            to: Account::from(bob()),
            amount: Tokens128::from(100),
            fee: None,
            memo: None,
            created_at_time: None,
        };
        assert_eq!(
            canister.icrc1_transfer(transfer1),
            Err(TxError::SelfTransfer)
        );
        assert_eq!(
            canister.icrc1_balance_of((alice(), None).into()),
            Tokens128::from(1000)
        );
        assert_eq!(
            canister.icrc1_balance_of((bob(), None).into()),
            Tokens128::from(0)
        );

        assert_eq!(
            canister.icrc1_balance_of((alice(), None).into()),
            Tokens128::from(1000)
        );
    }

    #[test]
    fn transfer_saved_into_history() {
        let (ctx, canister) = test_context();
        canister.state().borrow_mut().stats.fee = Tokens128::from(10);
        let transfer1 = TransferArgs {
            from_subaccount: None,
            to: Account::from(bob()),
            amount: Tokens128::from(1001),
            fee: None,
            memo: None,
            created_at_time: None,
        };

        canister.icrc1_transfer(transfer1).unwrap_err();
        assert_eq!(canister.historySize(), 1);

        const COUNT: u64 = 5;
        let mut ts = ic_canister::ic_kit::ic::time();
        for i in 0..COUNT {
            let transfer1 = TransferArgs {
                from_subaccount: None,
                to: Account::from(bob()),
                amount: Tokens128::from(100 + i as u128),
                fee: None,
                memo: None,
                created_at_time: None,
            };
            ctx.add_time(10);
            let id = canister.icrc1_transfer(transfer1).unwrap();
            assert_eq!(canister.historySize(), 2 + i);
            let tx = canister.getTransaction(id as u64);
            assert_eq!(tx.amount, Tokens128::from(100 + i as u128));
            assert_eq!(tx.fee, Tokens128::from(10));
            assert_eq!(tx.operation, Operation::Transfer);
            assert_eq!(tx.status, TransactionStatus::Succeeded);
            assert_eq!(tx.index, i + 1);
            assert_eq!(tx.from, alice().into());
            assert_eq!(tx.to, bob().into());
            assert!(ts < tx.timestamp);
            ts = tx.timestamp;
        }
    }

    #[test]
    fn mint_test_token() {
        let alice_sub = gen_subaccount();

        let canister = test_canister();
        MockContext::new().with_caller(bob()).inject();
        assert_eq!(
            canister.icrc1_mint(alice(), None, Tokens128::from(100)),
            Err(TxError::Unauthorized)
        );

        canister.state().borrow_mut().stats.is_test_token = true;

        assert!(canister
            .icrc1_mint(alice(), None, Tokens128::from(2000))
            .is_ok());
        assert!(canister
            .icrc1_mint(bob(), None, Tokens128::from(5000))
            .is_ok());

        assert_eq!(
            canister.icrc1_balance_of((alice(), None).into()),
            Tokens128::from(3000)
        );
        assert_eq!(
            canister.icrc1_balance_of((bob(), None).into()),
            Tokens128::from(5000)
        );
        assert!(canister
            .icrc1_mint(alice(), Some(alice_sub), Tokens128::from(1000))
            .is_ok());
        assert_eq!(
            canister.icrc1_balance_of((alice(), Some(alice_sub)).into()),
            Tokens128::from(1000)
        );
    }

    #[test]
    fn mint_by_owner() {
        let canister = test_canister();
        let alice_sub = gen_subaccount();
        let bob_sub = gen_subaccount();
        assert!(canister
            .icrc1_mint(alice(), None, Tokens128::from(2000))
            .is_ok());
        assert!(canister
            .icrc1_mint(bob(), None, Tokens128::from(5000))
            .is_ok());
        assert_eq!(
            canister.icrc1_balance_of((alice(), None).into()),
            Tokens128::from(3000)
        );
        assert_eq!(
            canister.icrc1_balance_of((bob(), None).into()),
            Tokens128::from(5000)
        );
        assert_eq!(canister.icrc1_total_supply(), Tokens128::from(8000));

        //     mint to subaccounts
        assert!(canister
            .icrc1_mint(alice(), Some(alice_sub), Tokens128::from(2000))
            .is_ok());
        assert!(canister
            .icrc1_mint(bob(), Some(bob_sub), Tokens128::from(5000))
            .is_ok());

        assert_eq!(
            canister.icrc1_balance_of((alice(), Some(alice_sub)).into()),
            Tokens128::from(2000)
        );
        assert_eq!(
            canister.icrc1_balance_of((bob(), Some(bob_sub)).into()),
            Tokens128::from(5000)
        );
        assert_eq!(canister.icrc1_total_supply(), Tokens128::from(15000));
    }

    #[test]
    fn mint_saved_into_history() {
        let (ctx, canister) = test_context();
        canister.state().borrow_mut().stats.fee = Tokens128::from(10);

        assert_eq!(canister.historySize(), 1);

        const COUNT: u64 = 5;
        let mut ts = ic_canister::ic_kit::ic::time();
        for i in 0..COUNT {
            ctx.add_time(10);
            let id = canister
                .icrc1_mint(bob(), None, Tokens128::from(100 + i as u128))
                .unwrap();
            assert_eq!(canister.historySize(), 2 + i);
            let tx = canister.getTransaction(id as u64);
            assert_eq!(tx.amount, Tokens128::from(100 + i as u128));
            assert_eq!(tx.fee, Tokens128::from(0));
            assert_eq!(tx.operation, Operation::Mint);
            assert_eq!(tx.status, TransactionStatus::Succeeded);
            assert_eq!(tx.index, i + 1);
            assert_eq!(tx.from, alice().into());
            assert_eq!(tx.to, bob().into());

            assert!(ts < tx.timestamp);
            ts = tx.timestamp;
        }
    }

    #[test]
    fn burn_by_owner() {
        let canister = test_canister();
        assert!(canister
            .icrc1_burn(None, None, Tokens128::from(100))
            .is_ok());
        assert_eq!(
            canister.icrc1_balance_of((alice(), None).into()),
            Tokens128::from(900)
        );
        assert_eq!(canister.icrc1_total_supply(), Tokens128::from(900));
    }

    #[test]
    fn burn_too_much() {
        let canister = test_canister();
        let balance = canister.icrc1_balance_of((alice(), None).into());
        assert_eq!(
            canister.icrc1_burn(None, None, Tokens128::from(1001)),
            Err(TxError::InsufficientFunds { balance })
        );
        assert_eq!(
            canister.icrc1_balance_of((alice(), None).into()),
            Tokens128::from(1000)
        );
        assert_eq!(canister.icrc1_total_supply(), Tokens128::from(1000));
    }

    #[test]
    fn burn_by_wrong_user() {
        let canister = test_canister();
        let context = MockContext::new().with_caller(bob()).inject();
        context.update_caller(bob());
        let balance = canister.icrc1_balance_of((bob(), None).into());
        assert_eq!(
            canister.icrc1_burn(None, None, Tokens128::from(100)),
            Err(TxError::InsufficientFunds { balance })
        );
        assert_eq!(
            canister.icrc1_balance_of((alice(), None).into()),
            Tokens128::from(1000)
        );
        assert_eq!(canister.icrc1_total_supply(), Tokens128::from(1000));
    }

    #[test]
    fn burn_from() {
        let bob_sub = gen_subaccount();
        let canister = test_canister();
        let bob_balance = Tokens128::from(1000);
        canister.icrc1_mint(bob(), None, bob_balance).unwrap();
        assert_eq!(canister.icrc1_balance_of((bob(), None).into()), bob_balance);
        canister
            .icrc1_burn(Some(bob()), None, Tokens128::from(100))
            .unwrap();
        assert_eq!(
            canister.icrc1_balance_of((bob(), None).into()),
            Tokens128::from(900)
        );
        assert_eq!(canister.icrc1_total_supply(), Tokens128::from(1900));
        //     Burn from subaccount
        canister
            .icrc1_mint(bob(), Some(bob_sub), bob_balance)
            .unwrap();
        assert_eq!(
            canister.icrc1_balance_of((bob(), Some(bob_sub)).into()),
            bob_balance
        );
        canister
            .icrc1_burn(Some(bob()), Some(bob_sub), Tokens128::from(100))
            .unwrap();
        assert_eq!(
            canister.icrc1_balance_of((bob(), Some(bob_sub)).into()),
            Tokens128::from(900)
        );
    }

    #[test]
    fn burn_from_unauthorized() {
        let canister = test_canister();
        let context = MockContext::new().with_caller(bob()).inject();
        context.update_caller(bob());
        assert_eq!(
            canister.icrc1_burn(Some(alice()), None, Tokens128::from(100)),
            Err(TxError::Unauthorized)
        );

        assert_eq!(
            canister.icrc1_balance_of((alice(), None).into()),
            Tokens128::from(1000)
        );
        assert_eq!(canister.icrc1_total_supply(), Tokens128::from(1000));
    }

    #[test]
    fn burn_saved_into_history() {
        let (ctx, canister) = test_context();
        canister.state().borrow_mut().stats.fee = Tokens128::from(10);

        canister
            .icrc1_burn(None, None, Tokens128::from(1001))
            .unwrap_err();
        assert_eq!(canister.historySize(), 1);

        const COUNT: u64 = 5;
        let mut ts = ic_canister::ic_kit::ic::time();
        for i in 0..COUNT {
            ctx.add_time(10);
            let id = canister
                .icrc1_burn(None, None, Tokens128::from(100 + i as u128))
                .unwrap();
            assert_eq!(canister.historySize(), 2 + i);
            let tx = canister.getTransaction(id as u64);
            assert_eq!(tx.amount, Tokens128::from(100 + i as u128));
            assert_eq!(tx.fee, Tokens128::from(0));
            assert_eq!(tx.operation, Operation::Burn);
            assert_eq!(tx.status, TransactionStatus::Succeeded);
            assert_eq!(tx.index, i + 1);
            assert_eq!(tx.to, alice().into());
            assert_eq!(tx.from, alice().into());
            assert!(ts < tx.timestamp);
            ts = tx.timestamp;
        }
    }

    #[test]
    fn get_transactions_test() {
        let canister = test_canister();
        let transfer1 = TransferArgs {
            from_subaccount: None,
            to: Account::from(bob()),
            amount: Tokens128::from(10),
            fee: None,
            memo: None,
            created_at_time: None,
        };

        for _ in 1..=5 {
            canister.icrc1_transfer(transfer1.clone()).unwrap();
        }
        let transfer2 = TransferArgs {
            from_subaccount: None,
            to: Account::from(bob()),

            amount: Tokens128::from(10),
            fee: None,
            memo: None,
            created_at_time: None,
        };
        canister.icrc1_transfer(transfer2).unwrap();
        let transfer3 = TransferArgs {
            from_subaccount: None,
            to: Account::from(xtc()),
            amount: Tokens128::from(10),
            fee: None,
            memo: None,
            created_at_time: None,
        };
        canister.icrc1_transfer(transfer3).unwrap();
        let transfer4 = TransferArgs {
            from_subaccount: None,
            to: Account::from(john()),
            amount: Tokens128::from(10),
            fee: None,
            memo: None,
            created_at_time: None,
        };
        canister.icrc1_transfer(transfer4).unwrap();

        assert_eq!(canister.getTransactions(None, 10, None).result.len(), 9);
        assert_eq!(canister.getTransactions(None, 10, Some(3)).result.len(), 4);
        assert_eq!(
            canister.getTransactions(Some(bob()), 10, None).result.len(),
            6
        );
        assert_eq!(
            canister.getTransactions(Some(xtc()), 5, None).result.len(),
            1
        );
        assert_eq!(
            canister
                .getTransactions(Some(alice()), 10, Some(5))
                .result
                .len(),
            6
        );
        assert_eq!(canister.getTransactions(None, 5, None).next, Some(3));
        assert_eq!(
            canister.getTransactions(Some(alice()), 3, Some(5)).next,
            Some(2)
        );
        assert_eq!(canister.getTransactions(Some(bob()), 3, Some(2)).next, None);

        let transfer5 = TransferArgs {
            from_subaccount: None,
            to: Account::from(bob()),
            amount: Tokens128::from(10),
            fee: None,
            memo: None,
            created_at_time: None,
        };

        for _ in 1..=10 {
            canister.icrc1_transfer(transfer5.clone()).unwrap();
        }

        let txn = canister.getTransactions(None, 5, None);
        assert_eq!(txn.result[0].index, 18);
        assert_eq!(txn.result[1].index, 17);
        assert_eq!(txn.result[2].index, 16);
        assert_eq!(txn.result[3].index, 15);
        assert_eq!(txn.result[4].index, 14);
        let txn2 = canister.getTransactions(None, 5, txn.next);
        assert_eq!(txn2.result[0].index, 13);
        assert_eq!(txn2.result[1].index, 12);
        assert_eq!(txn2.result[2].index, 11);
        assert_eq!(txn2.result[3].index, 10);
        assert_eq!(txn2.result[4].index, 9);
        assert_eq!(canister.getTransactions(None, 5, txn.next).next, Some(8));
    }

    #[test]
    #[should_panic]
    fn get_transaction_not_existing() {
        let canister = test_canister();
        canister.getTransaction(2);
    }

    #[test]
    fn get_transaction_count() {
        let canister = test_canister();
        const COUNT: usize = 10;
        let transfer1 = TransferArgs {
            from_subaccount: None,
            to: Account::from(bob()),

            amount: Tokens128::from(10),
            fee: None,
            memo: None,
            created_at_time: None,
        };
        for _ in 1..COUNT {
            canister.icrc1_transfer(transfer1.clone()).unwrap();
        }
        assert_eq!(canister.getUserTransactionCount(alice()), COUNT);
    }

    #[test]
    fn mint_to_account_id() {
        let subaccount = gen_subaccount();
        let alice_aid =
            AccountIdentifier::new(alice().into(), Some(SubaccountIdentifier(subaccount)));

        let canister = test_canister();
        assert!(canister
            .mintToAccountId(alice_aid, Tokens128::from(100))
            .is_ok());
        assert!(canister.claim(alice_aid, Some(subaccount)).is_ok());
        assert_eq!(
            canister.icrc1_balance_of((alice(), Some(subaccount)).into()),
            Tokens128::from(100)
        );
        assert_eq!(canister.icrc1_total_supply(), Tokens128::from(1100));
        assert_eq!(canister.state().borrow().claims.len(), 0);
    }

    #[test]
    fn test_claim_amount() {
        let bob_sub = gen_subaccount();
        let alice_sub = gen_subaccount();

        let alice_aid =
            AccountIdentifier::new(alice().into(), Some(SubaccountIdentifier(alice_sub)));
        let bob_aid = AccountIdentifier::new(bob().into(), Some(SubaccountIdentifier(bob_sub)));

        let canister = test_canister();

        assert!(canister
            .mintToAccountId(alice_aid, Tokens128::from(1000))
            .is_ok());
        assert!(canister
            .mintToAccountId(bob_aid, Tokens128::from(2000))
            .is_ok());
        assert_eq!(
            canister.getClaim(Some(alice_sub)).unwrap(),
            Tokens128::from(1000)
        );
        MockContext::new().with_caller(bob()).inject();
        assert_eq!(
            canister.getClaim(Some(bob_sub)).unwrap(),
            Tokens128::from(2000)
        );
    }

    #[test]
    fn valid_transaction_time_window() {
        let canister = test_canister();

        let system_time = std::time::SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();

        let transfer = TransferArgs {
            from_subaccount: None,

            to: Account::from(bob()),
            amount: Tokens128::from(10),
            fee: None,
            memo: None,
            created_at_time: Some(system_time as u64 + 30_000_000_000),
        };
        assert!(canister.icrc1_transfer(transfer).is_ok());
    }

    #[test]
    fn invalid_transaction_time_window() {
        let canister = test_canister();

        let system_time = std::time::SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();

        let transfer = TransferArgs {
            from_subaccount: None,
            to: Account::from(bob()),
            amount: Tokens128::from(10),
            fee: None,
            memo: None,
            created_at_time: Some(system_time as u64 - ONE_MIN_IN_NANOS * 2),
        };
        assert!(canister.icrc1_transfer(transfer).is_err());

        let transfer = TransferArgs {
            from_subaccount: None,
            to: Account::from(bob()),
            amount: Tokens128::from(10),
            fee: None,
            memo: None,
            created_at_time: Some(system_time as u64 + ONE_MIN_IN_NANOS * 2),
        };
        assert!(canister.icrc1_transfer(transfer).is_err());
    }

    #[test]
    fn test_invalid_self_account_transfer() {
        let canister = test_canister();
        assert_eq!(
            canister.icrc1_balance_of((alice(), None).into()),
            Tokens128::from(1000)
        );
        let transfer = TransferArgs {
            from_subaccount: None,
            to: Account::from(alice()),
            amount: Tokens128::from(100),
            fee: None,
            memo: None,
            created_at_time: None,
        };
        assert!(canister.icrc1_transfer(transfer).is_err());

        assert_eq!(
            canister.icrc1_balance_of((alice(), None).into()),
            Tokens128::from(1000)
        );

        let alice_sub = gen_subaccount();

        let transfer = TransferArgs {
            from_subaccount: Some(alice_sub),
            to: Account::new(alice(), Some(alice_sub)),
            amount: Tokens128::from(100),
            fee: None,
            memo: None,
            created_at_time: None,
        };

        assert!(canister.icrc1_transfer(transfer.clone()).is_err());

        assert_eq!(
            canister.icrc1_balance_of((alice(), Some(alice_sub)).into()),
            Tokens128::from(0)
        );

        assert_eq!(
            canister.icrc1_transfer(transfer),
            Err(TxError::SelfTransfer)
        );
    }

    #[test]
    fn test_valid_self_subaccount_transfer() {
        let canister = test_canister();
        let alice_sub1 = gen_subaccount();
        assert_eq!(
            canister.icrc1_balance_of((alice(), None).into()),
            Tokens128::from(1000)
        );
        let transfer = TransferArgs {
            from_subaccount: None,
            to: Account::new(alice(), Some(alice_sub1)),

            amount: Tokens128::from(100),
            fee: None,
            memo: None,
            created_at_time: None,
        };
        assert!(canister.icrc1_transfer(transfer).is_ok());

        assert_eq!(
            canister.icrc1_balance_of((alice(), None).into()),
            Tokens128::from(900)
        );
        assert_eq!(
            canister.icrc1_balance_of((alice(), Some(alice_sub1)).into()),
            Tokens128::from(100)
        );

        let alice_sub2 = gen_subaccount();

        let transfer = TransferArgs {
            from_subaccount: Some(alice_sub1),
            to: Account::new(alice(), Some(alice_sub2)),
            amount: Tokens128::from(10),
            fee: None,
            memo: None,
            created_at_time: None,
        };
        assert!(canister.icrc1_transfer(transfer).is_ok());
        assert_eq!(
            canister.icrc1_balance_of((alice(), Some(alice_sub2)).into()),
            Tokens128::from(10)
        );
        assert_eq!(
            canister.icrc1_balance_of((alice(), Some(alice_sub1)).into()),
            Tokens128::from(90)
        );
    }
}

#[cfg(test)]
mod proptests {
    use ic_canister::ic_kit::MockContext;
    use ic_canister::Canister;
    use proptest::collection::vec;
    use proptest::prelude::*;
    use proptest::sample::Index;

    use crate::mock::*;
    use crate::types::Metadata;

    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Action {
        Mint {
            minter: Principal,
            recipient: Principal,
            amount: Tokens128,
        },
        Burn(Tokens128, Principal),
        TransferWithFee {
            from: Principal,
            to: Principal,
            amount: Tokens128,
        },
        TransferWithoutFee {
            from: Principal,
            to: Principal,
            amount: Tokens128,
            fee_limit: Option<Tokens128>,
        },
    }

    prop_compose! {
        fn select_principal(p: Vec<Principal>) (index in any::<Index>()) -> Principal {
            let i = index.index(p.len());
            p[i]
        }

    }

    fn make_action(principals: Vec<Principal>) -> impl Strategy<Value = Action> {
        prop_oneof![
            // Mint
            (
                make_tokens128(),
                select_principal(principals.clone()),
                select_principal(principals.clone()),
            )
                .prop_map(|(amount, minter, recipient)| Action::Mint {
                    minter,
                    recipient,
                    amount
                }),
            // Burn
            (make_tokens128(), select_principal(principals.clone()))
                .prop_map(|(amount, principal)| Action::Burn(amount, principal)),
            // With fee
            (
                select_principal(principals.clone()),
                select_principal(principals.clone()),
                make_tokens128()
            )
                .prop_map(|(from, to, amount)| Action::TransferWithFee {
                    from,
                    to,
                    amount
                }),
            // Without fee
            (
                select_principal(principals.clone()),
                select_principal(principals),
                make_tokens128(),
                make_option(),
            )
                .prop_map(|(from, to, amount, fee_limit)| {
                    Action::TransferWithoutFee {
                        from,
                        to,
                        amount,
                        fee_limit,
                    }
                }),
            // Transfer from
        ]
    }

    fn make_option() -> impl Strategy<Value = Option<Tokens128>> {
        prop_oneof![Just(None), (make_tokens128()).prop_map(Some)]
    }

    fn make_principal() -> BoxedStrategy<Principal> {
        (any::<[u8; 29]>().prop_map(|mut bytes| {
            // Make sure the last byte is more than four as the last byte carries special
            // meaning
            bytes[28] = bytes[28].saturating_add(5);
            bytes
        }))
        .prop_map(|bytes| Principal::from_slice(&bytes))
        .boxed()
    }

    prop_compose! {
        fn make_tokens128() (num in "[0-9]{1,10}") -> Tokens128 {
            Tokens128::from(num.parse::<u128>().unwrap())
        }
    }
    prop_compose! {
        fn make_canister() (
            logo in any::<String>(),
            name in any::<String>(),
            symbol in any::<String>(),
            decimals in any::<u8>(),
            total_supply in make_tokens128(),
            fee in make_tokens128(),
            principals in vec(make_principal(), 1..7),
            owner_idx in any::<Index>(),
            fee_to_idx in any::<Index>(),
        )-> (TokenCanisterMock, Vec<Principal>) {
            // pick two random principals (they could very well be the same principal twice)
            let owner = principals[owner_idx.index(principals.len())];
            let fee_to = principals[fee_to_idx.index(principals.len())];
            MockContext::new().with_caller(owner).inject();
            let meta = Metadata {
                logo,
                name,
                symbol,
                decimals,
                owner,
                fee,
                feeTo: fee_to,
                isTestToken: None,
            };
            let canister = TokenCanisterMock::init_instance();
            canister.init(meta,total_supply);
            // This is to make tests that don't rely on auction state
            // pass, because since we are running auction state on each
            // endpoint call, it affects `BiddingInfo.fee_ratio` that is
            // used for charging fees in `approve` endpoint.
            canister.state.borrow_mut().stats.min_cycles = 0;
            (canister, principals)
        }
    }
    fn canister_and_actions() -> impl Strategy<Value = (TokenCanisterMock, Vec<Action>)> {
        make_canister().prop_flat_map(|(canister, principals)| {
            let actions = vec(make_action(principals), 1..7);
            (Just(canister), actions)
        })
    }
    proptest! {
        #[test]
        fn generic_proptest((canister, actions) in canister_and_actions()) {
            let mut total_minted = Tokens128::ZERO;
            let mut total_burned = Tokens128::ZERO;
            let starting_supply = canister.icrc1_total_supply();
            for action in actions {
                use Action::*;
                match action {
                    Mint { minter, recipient, amount } => {
                        MockContext::new().with_caller(minter).inject();
                        let original = canister.icrc1_total_supply();
                        let res = canister.icrc1_mint(recipient, None,amount);
                        let expected = if minter == canister.owner() {
                            total_minted = (total_minted + amount).unwrap();
                            assert!(matches!(res, Ok(_)));
                            (original + amount).unwrap()
                        } else {
                            assert_eq!(res, Err(TxError::Unauthorized));
                            original
                        };
                        assert_eq!(expected, canister.icrc1_total_supply());
                    },
                    Burn(amount, burner) => {
                        MockContext::new().with_caller(burner).inject();
                        let original = canister.icrc1_total_supply();
                        let balance = canister.icrc1_balance_of((burner,None).into());
                        let res = canister.icrc1_burn(Some(burner), None, amount);
                        if balance < amount {
                            prop_assert_eq!(res, Err(TxError::InsufficientFunds { balance }));
                            prop_assert_eq!(original, canister.icrc1_total_supply());
                        } else {
                            prop_assert!(matches!(res, Ok(_)), "Burn error: {:?}. Balance: {}, amount: {}", res, balance, amount);
                            prop_assert_eq!((original - amount).unwrap(), canister.icrc1_total_supply());
                            total_burned = (total_burned + amount).unwrap();
                        }
                    },

                    TransferWithoutFee{from,to,amount,fee_limit} => {
                        MockContext::new().with_caller(from).inject();
                        let from_balance = canister.icrc1_balance_of((from, None).into());
                        let to_balance = canister.icrc1_balance_of((to, None).into());
                        let (fee , fee_to) = canister.state().borrow().stats.fee_info();
                        let amount_with_fee = (amount + fee).unwrap();
                        let transfer1 = TransferArgs {
                            from_subaccount: None,
                            to:Account::new(to, None),
                            amount,
                            fee: fee_limit,
                            memo: None,
                            created_at_time: None,
                        };
                        let res = canister.icrc1_transfer(transfer1);

                        if to == from {
                            prop_assert_eq!(res, Err(TxError::SelfTransfer));
                            return Ok(())
                        }

                        if let Some(fee_limit) = fee_limit {
                            if fee_limit < fee {
                                prop_assert_eq!(res, Err(TxError::FeeExceededLimit));
                                return Ok(())
                            }
                        }

                        if from_balance < amount_with_fee {
                            prop_assert_eq!(res, Err(TxError::InsufficientFunds { balance:from_balance }));
                            return Ok(())
                        }

                        if fee_to == from  {
                            prop_assert!(matches!(res, Ok(_)));
                            prop_assert_eq!((from_balance - amount).unwrap(), canister.icrc1_balance_of((from, None).into()));
                            return Ok(());
                        }

                        if fee_to == to  {
                            prop_assert!(matches!(res, Ok(_)));
                            prop_assert_eq!(((to_balance + amount).unwrap() + fee).unwrap(), canister.icrc1_balance_of((to,None).into()));
                            return Ok(());
                        }

                        prop_assert!(matches!(res, Ok(_)));
                        prop_assert_eq!((from_balance - amount_with_fee).unwrap(), canister.icrc1_balance_of((from, None).into()));
                        prop_assert_eq!((to_balance + amount).unwrap(), canister.icrc1_balance_of((to, None).into()));

                    }
                    TransferWithFee { from, to, amount } => {
                        MockContext::new().with_caller(from).inject();
                        let from_balance = canister.icrc1_balance_of((from,None).into());
                        let to_balance = canister.icrc1_balance_of((to,None).into());
                        let (fee , fee_to) = canister.state().borrow().stats.fee_info();
                        let res = canister.icrc1_transferIncludeFee(None, to, None, amount,None,None);

                        if to == from {
                            prop_assert_eq!(res, Err(TxError::SelfTransfer));
                            return Ok(())
                        }

                        if amount <= fee  {
                            prop_assert_eq!(res, Err(TxError::AmountTooSmall));
                            return Ok(());
                        }
                        if from_balance < amount {
                            prop_assert_eq!(res, Err(TxError::InsufficientFunds { balance: from_balance }));
                            return Ok(());
                        }

                        // Sometimes the fee can be sent `to` or `from`
                        if fee_to == from  {
                            prop_assert_eq!(((from_balance - amount).unwrap() + fee).unwrap(), canister.icrc1_balance_of((from,None).into()));
                            return Ok(());
                        }

                        if fee_to == to  {
                            prop_assert_eq!((to_balance + amount).unwrap(), canister.icrc1_balance_of((to,None).into()));
                            return Ok(());
                        }

                        prop_assert!(matches!(res, Ok(_)));
                        prop_assert_eq!(((to_balance + amount).unwrap() - fee).unwrap(), canister.icrc1_balance_of((to,None).into()));
                        prop_assert_eq!((from_balance - amount).unwrap(), canister.icrc1_balance_of((from,None).into()));

                    }
                }
            }
            prop_assert_eq!(((total_minted + starting_supply).unwrap() - total_burned).unwrap(), canister.icrc1_total_supply());
        }
    }
}
