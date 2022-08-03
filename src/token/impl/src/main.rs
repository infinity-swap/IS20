#![allow(dead_code)]

mod canister;

#[cfg(any(target_arch = "wasm32", test))]
fn main() {}

#[cfg(not(any(target_arch = "wasm32", test)))]
fn main() {
    use crate::canister::TokenCanister;
    use ic_helpers::candid_header::CandidHeader;
    use ic_helpers::tokens::Tokens128;
    use token_api::canister::TokenCanisterAPI;
    use token_api::types::Metadata;

    let canister_idl = ic_canister::generate_idl!();
    let mut trait_idl = <TokenCanister as TokenCanisterAPI>::get_idl();
    trait_idl.merge(&canister_idl);

    let result = candid::bindings::candid::compile(&trait_idl.env.env, &Some(trait_idl.actor));
    print!("{result}");
}
