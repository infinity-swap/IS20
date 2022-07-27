//! Module     : factory
//! Copyright  : 2022 InfinitySwap Team
//! Stability  : Experimental

use std::cell::RefCell;
use std::rc::Rc;

use crate::state::StableState;
use crate::{error::TokenFactoryError, state::State};
use candid::Principal;
use ic_canister::{init, post_upgrade, pre_upgrade, query, update, Canister, PreUpdate};
use ic_factory::{api::FactoryCanister, error::FactoryError, FactoryConfiguration, FactoryState};
use ic_helpers::candid_header::{candid_header, CandidHeader};
use token::types::Metadata;

const DEFAULT_LEDGER_PRINCIPAL: &str = "ryjl3-tyaaa-aaaaa-aaaba-cai";
const DEFAULT_ICP_FEE: u64 = 10u64.pow(8); // 1 ICP

#[cfg(not(feature = "no_api"))]
mod inspect_message;

#[derive(Clone, Canister)]
#[canister_no_upgrade_methods]
pub struct TokenFactoryCanister {
    #[id]
    principal: Principal,

    #[state]
    pub state: Rc<RefCell<State>>,
}

#[allow(dead_code)]
impl TokenFactoryCanister {
    #[query]
    fn git_tag(&self) -> &'static str {
        option_env!("GIT_TAG").unwrap_or("NOT_FOUND")
    }

    #[pre_upgrade]
    fn pre_upgrade(&self) {
        // Default states just to chek that the storage is writeable.
        let token_factory_state = self.state.replace(State::default());
        let base_factory_state = self.factory_state().replace(FactoryState::default());

        ic_storage::stable::write(&StableState {
            token_factory_state,
            base_factory_state,
        })
        .expect("failed to serialize state to the stable storage");
    }

    #[post_upgrade]
    fn post_upgrade(&self) {
        let stable_state = ic_storage::stable::read::<StableState>()
            .expect("failed to read stable state from the stable storage");
        let StableState {
            token_factory_state,
            base_factory_state,
        } = stable_state;

        self.state.replace(token_factory_state);
        self.factory_state().replace(base_factory_state);
    }

    #[init]
    pub fn init(&self, controller: Principal, ledger_principal: Option<Principal>) {
        let ledger = ledger_principal
            .unwrap_or_else(|| Principal::from_text(DEFAULT_LEDGER_PRINCIPAL).unwrap());

        let factory_configuration =
            FactoryConfiguration::new(ledger, DEFAULT_ICP_FEE, controller, controller);
        ic_cdk::println!("factory controller: {}", controller.to_string());

        self.factory_state()
            .replace(FactoryState::new(factory_configuration));
    }

    /// Returns the token, or None if it does not exist.
    #[query]
    pub async fn get_token(&self, name: String) -> Option<Principal> {
        self.state.borrow().tokens.get(&name).copied()
    }

    #[update]
    pub async fn set_token_bytecode(
        &self,
        bytecode: Vec<u8>,
        state_header: CandidHeader,
    ) -> Result<u32, FactoryError> {
        self.set_canister_code::<token::state::CanisterState>(bytecode, state_header)
    }

    /// Creates a new token.
    ///
    /// Creating a token canister with the factory requires one of the following:
    /// * the call must be made through a cycles wallet with enough cycles to cover the canister
    ///   expenses. The amount of provided cycles must be greater than `10^12`. Most of the cycles
    ///   will be added to the newly created canister balance, while some will be consumed by the
    ///   factory
    /// * the caller must transfer some amount of ICP to their subaccount into the ICP ledger factory account.
    ///   The subaccount id can be calculated like this:
    ///
    /// ```ignore
    /// let mut subaccount = [0u8; 32];
    /// let principal_id = caller_id.as_slice();
    /// subaccount[0] = principal_id.len().try_into().unwrap();
    /// subaccount[1..1 + principal_id.len()].copy_from_slice(principal_id);
    /// ```
    ///
    /// The amount of provided ICP must be greater than the `icp_fee` factory property. This value
    /// can be obtained by the `get_icp_fee` query method. The ICP fees are transferred to the
    /// principal designated by the factory controller. The canister is then created with some
    /// minimum amount of cycles.
    ///
    /// If the provided ICP amount is greater than required by the factory, extra ICP will not be
    /// consumed and can be used to create more canisters, or can be reclaimed by calling `refund_icp`
    /// method.
    #[update]
    pub async fn create_token(
        &self,
        info: Metadata,
        controller: Option<Principal>,
    ) -> Result<Principal, TokenFactoryError> {
        if info.name.is_empty() {
            return Err(TokenFactoryError::InvalidConfiguration(
                "name",
                "cannot be `None`",
            ));
        }

        if info.symbol.is_empty() {
            return Err(TokenFactoryError::InvalidConfiguration(
                "symbol",
                "cannot be `None`",
            ));
        }

        let key = info.name.clone();
        if self.state.borrow().tokens.contains_key(&key) {
            return Err(TokenFactoryError::AlreadyExists);
        }

        let caller = ic_canister::ic_kit::ic::caller();

        let principal = self
            .create_canister((info,), controller, Some(caller))
            .await?;
        self.state.borrow_mut().tokens.insert(key, principal);

        Ok(principal)
    }

    #[update]
    pub async fn forget_token(&self, name: String) -> Result<(), TokenFactoryError> {
        let canister_id = self
            .get_token(name.clone())
            .await
            .ok_or(TokenFactoryError::FactoryError(FactoryError::NotFound))?;

        self.drop_canister(canister_id, None).await?;
        self.state.borrow_mut().tokens.remove(&name);

        Ok(())
    }

    #[update]
    pub async fn upgrade(
        &mut self,
    ) -> Result<std::collections::HashMap<Principal, ic_factory::api::UpgradeResult>, FactoryError>
    {
        self.upgrade_canister::<token::state::CanisterState>().await
    }

    #[query]
    pub fn state_header(&self) -> CandidHeader {
        candid_header::<StableState>()
    }
}

impl PreUpdate for TokenFactoryCanister {}
impl FactoryCanister for TokenFactoryCanister {}
