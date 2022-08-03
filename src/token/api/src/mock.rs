use std::{cell::RefCell, rc::Rc};

use candid::Principal;
use ic_canister::{Canister, PreUpdate};
use ic_helpers::tokens::Tokens128;

use crate::{canister::TokenCanisterAPI, state::CanisterState, types::Metadata};

#[derive(Debug, Clone, Canister)]
pub struct TokenCanisterMock {
    #[id]
    principal: Principal,
    #[state]
    pub(crate) state: Rc<RefCell<CanisterState>>,
}

impl TokenCanisterMock {
    pub fn init(&self, metadata: Metadata, amount: Tokens128) {
        self.state
            .borrow_mut()
            .balances
            .insert(metadata.owner, None, amount);

        self.state
            .borrow_mut()
            .ledger
            .mint(metadata.owner.into(), metadata.owner.into(), amount);

        self.state.borrow_mut().stats = metadata.into();
        self.state.borrow_mut().bidding_state.auction_period =
            crate::canister::DEFAULT_AUCTION_PERIOD;
    }
}

impl PreUpdate for TokenCanisterMock {
    fn pre_update(&self, method_name: &str, method_type: ic_canister::MethodType) {
        crate::canister::pre_update(self, method_name, method_type);
    }
}

impl TokenCanisterAPI for TokenCanisterMock {
    fn state(&self) -> Rc<RefCell<CanisterState>> {
        self.state.clone()
    }
}
