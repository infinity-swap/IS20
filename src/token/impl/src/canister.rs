use candid::Principal;
use ic_canister::{init, query, Canister, PreUpdate};

#[cfg(not(feature = "no_api"))]
use ic_cdk_macros::inspect_message;

use ic_helpers::{
    candid_header::{candid_header, CandidHeader},
    tokens::Tokens128,
};
use std::{cell::RefCell, rc::Rc};
use token_api::{
    account::Account,
    canister::{TokenCanisterAPI, DEFAULT_AUCTION_PERIOD},
    state::CanisterState,
    types::Metadata,
};

#[derive(Debug, Clone, Canister)]
pub struct TokenCanister {
    #[id]
    principal: Principal,
    #[state]
    pub(crate) state: Rc<RefCell<CanisterState>>,
}

impl TokenCanister {
    #[init]
    pub fn init(&self, metadata: Metadata, amount: Tokens128) {
        self.state
            .borrow_mut()
            .balances
            .insert(metadata.owner, None, amount);

        self.state.borrow_mut().ledger.mint(
            Account::from(metadata.owner),
            Account::from(metadata.owner),
            amount,
        );

        self.state.borrow_mut().stats = metadata.into();
        self.state.borrow_mut().bidding_state.auction_period = DEFAULT_AUCTION_PERIOD;
    }

    #[query]
    pub fn state_check(&self) -> CandidHeader {
        candid_header::<CanisterState>()
    }
}

#[cfg(not(feature = "no_api"))]
#[inspect_message]
fn inspect_message() {
    use ic_storage::IcStorage;
    use token_api::canister::AcceptReason;

    let method = ic_cdk::api::call::method_name();

    let state = CanisterState::get();
    let state = state.borrow();
    let caller = ic_cdk::api::caller();

    let accept_reason = match TokenCanister::inspect_message(&state, &method, caller) {
        Ok(accept_reason) => accept_reason,
        Err(msg) => ic_cdk::trap(msg),
    };

    match accept_reason {
        AcceptReason::Valid => ic_cdk::api::call::accept_message(),
        AcceptReason::NotIS20Method => ic_cdk::trap("Unknown method"),
    }
}

impl PreUpdate for TokenCanister {
    fn pre_update(&self, method_name: &str, method_type: ic_canister::MethodType) {
        token_api::canister::pre_update(self, method_name, method_type);
    }
}

impl TokenCanisterAPI for TokenCanister {
    fn state(&self) -> Rc<RefCell<CanisterState>> {
        self.state.clone()
    }
}

#[cfg(test)]
mod test {
    use ic_canister::ic_kit::MockContext;

    use super::*;

    #[test]
    fn test_upgrade_from_previous() {
        use ic_storage::stable::write;

        MockContext::new().inject();

        write(&()).unwrap();
        let canister = TokenCanister::init_instance();
        canister.__post_upgrade_inst();
    }

    #[test]
    fn test_upgrade_from_current() {
        MockContext::new().inject();

        // Set a value on the state...
        let canister = TokenCanister::init_instance();
        let mut state = canister.state.borrow_mut();
        state.bidding_state.fee_ratio = 12345.0;
        drop(state);
        // ... write the state to stable storage
        canister.__pre_upgrade_inst();

        // Update the value without writing it to stable storage
        let mut state = canister.state.borrow_mut();
        state.bidding_state.fee_ratio = 0.0;
        drop(state);

        // Upgrade the canister should have the state
        // written before pre_upgrade
        canister.__post_upgrade_inst();
        let state = canister.state.borrow();
        assert_eq!(state.bidding_state.fee_ratio, 12345.0);
    }
}
