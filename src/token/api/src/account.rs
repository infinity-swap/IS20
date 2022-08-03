use std::fmt::{Display, Formatter};

use candid::{CandidType, Principal};
use serde::{Deserialize, Serialize};

use crate::types::TxError;

pub static DEFAULT_SUBACCOUNT: Subaccount = [0u8; 32];

#[derive(Debug, Clone, CandidType, Deserialize, Copy, PartialEq, Eq, Serialize)]
pub struct Account {
    pub principal: Principal,
    pub subaccount: Option<Subaccount>,
}

impl Account {
    pub fn new(principal: Principal, subaccount: Option<Subaccount>) -> Self {
        Self {
            principal,
            subaccount,
        }
    }
}

impl From<Principal> for Account {
    fn from(principal: Principal) -> Self {
        Self::new(principal, None)
    }
}

impl Display for Account {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        write!(f, "{}", self.principal)
    }
}

pub type Subaccount = [u8; 32];

pub struct CheckedAccount<T>(Account, T);

impl<T> CheckedAccount<T> {
    pub fn inner(&self) -> Account {
        self.0
    }

    pub fn principal(&self) -> Principal {
        self.0.principal
    }

    pub fn subaccount(&self) -> Subaccount {
        self.0.subaccount.unwrap_or(DEFAULT_SUBACCOUNT)
    }
}

pub struct WithRecipient {
    pub recipient: Account,
}

impl CheckedAccount<WithRecipient> {
    pub fn with_recipient(
        recipient: Account,
        from_subaccount: Option<Subaccount>,
    ) -> Result<Self, TxError> {
        let caller = ic_canister::ic_kit::ic::caller();
        let from = Account::new(caller, from_subaccount);
        if recipient == from {
            Err(TxError::SelfTransfer)
        } else {
            Ok(Self(from, WithRecipient { recipient }))
        }
    }
    pub fn recipient(&self) -> Account {
        self.1.recipient
    }
}
