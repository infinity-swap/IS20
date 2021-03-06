use crate::types::{Operation, TransactionStatus, TxId};
use candid::{CandidType, Deserialize, Principal};
use ic_canister::ic_kit::ic;
use ic_helpers::tokens::Tokens128;

#[derive(Deserialize, CandidType, Debug, Clone)]
pub struct TxRecord {
    pub caller: Option<Principal>,
    pub index: TxId,
    pub from: Principal,
    pub to: Principal,
    pub amount: Tokens128,
    pub fee: Tokens128,
    pub timestamp: u64,
    pub status: TransactionStatus,
    pub operation: Operation,
}

impl TxRecord {
    pub fn transfer(
        index: TxId,
        from: Principal,
        to: Principal,
        amount: Tokens128,
        fee: Tokens128,
    ) -> Self {
        Self {
            caller: Some(from),
            index,
            from,
            to,
            amount,
            fee,
            timestamp: ic::time(),
            status: TransactionStatus::Succeeded,
            operation: Operation::Transfer,
        }
    }

    pub fn transfer_from(
        index: TxId,
        caller: Principal,
        from: Principal,
        to: Principal,
        amount: Tokens128,
        fee: Tokens128,
    ) -> Self {
        Self {
            caller: Some(caller),
            index,
            from,
            to,
            amount,
            fee,
            timestamp: ic::time(),
            status: TransactionStatus::Succeeded,
            operation: Operation::TransferFrom,
        }
    }

    pub fn approve(
        index: TxId,
        from: Principal,
        to: Principal,
        amount: Tokens128,
        fee: Tokens128,
    ) -> Self {
        Self {
            caller: Some(from),
            index,
            from,
            to,
            amount,
            fee,
            timestamp: ic::time(),
            status: TransactionStatus::Succeeded,
            operation: Operation::Approve,
        }
    }

    pub fn mint(index: TxId, from: Principal, to: Principal, amount: Tokens128) -> Self {
        Self {
            caller: Some(from),
            index,
            from,
            to,
            amount,
            fee: Tokens128::from(0u128),
            timestamp: ic::time(),
            status: TransactionStatus::Succeeded,
            operation: Operation::Mint,
        }
    }

    pub fn burn(index: TxId, caller: Principal, from: Principal, amount: Tokens128) -> Self {
        Self {
            caller: Some(caller),
            index,
            from,
            to: from,
            amount,
            fee: Tokens128::from(0u128),
            timestamp: ic::time(),
            status: TransactionStatus::Succeeded,
            operation: Operation::Burn,
        }
    }

    pub fn auction(index: TxId, to: Principal, amount: Tokens128) -> Self {
        Self {
            caller: Some(to),
            index,
            from: to,
            to,
            amount,
            fee: Tokens128::from(0u128),
            timestamp: ic::time(),
            status: TransactionStatus::Succeeded,
            operation: Operation::Auction,
        }
    }
}
