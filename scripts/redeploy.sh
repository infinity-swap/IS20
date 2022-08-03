set -e
mkdir -p .dfx/${NETWORK} $HOME/.config/dfx/identity/max
echo "${CONTROLLER_IDENTITY}" >$HOME/.config/dfx/identity/max/identity.pem
export CANISTER_ID=$(curl -s https://${NETWORK}.infinityswap.one/canister_id/token_factory)
echo "{\"token_factory\":{\"${NETWORK}\":\"${CANISTER_ID}\"}}" >.dfx/${NETWORK}/canister_ids.json
dfx identity --network ${NETWORK} use max
export WALLET=$(dfx identity --network ${NETWORK} get-wallet)
export CONTROLLER_PRINCIPAL=$(dfx identity --network ${NETWORK} get-principal)
dfx deploy --wallet ${WALLET} --network ${NETWORK} --argument "(principal \"$CONTROLLER_PRINCIPAL\", null)" token_factory
dfx canister --wallet ${WALLET} --network ${NETWORK} call token_factory upgrade
dfx deploy --wallet ${WALLET} --network ${NETWORK} --argument 'record {logo = ""; name = "y"; symbol = "y"; decimals = 8; total_supply = 1000000000; owner = principal "aaaaa-aa"; fee = 0; feeTo = principal "aaaaa-aa";}' token
curl https://${NETWORK}.infinityswap.one/update --data-urlencode "path=/var/dfx-server/.dfx/local/canisters/token_factory/token_factory.did.js" --data-urlencode content@.dfx/${NETWORK}/canisters/token_factory/token_factory.did.js -u ${BASIC_AUTH_USERNAME}:${BASIC_AUTH_PASSWORD}
curl https://${NETWORK}.infinityswap.one/update --data-urlencode "path=/var/dfx-server/.dfx/local/canisters/token/token.did.js" --data-urlencode content@.dfx/${NETWORK}/canisters/token/token.did.js -u ${BASIC_AUTH_USERNAME}:${BASIC_AUTH_PASSWORD}
curl https://${NETWORK}.infinityswap.one/update --data-urlencode "path=/var/dfx-server/candid/token-factory.did" --data-urlencode content@src/candid/token-factory.did -u ${BASIC_AUTH_USERNAME}:${BASIC_AUTH_PASSWORD}
curl https://${NETWORK}.infinityswap.one/update --data-urlencode "path=/var/dfx-server/candid/token.did" --data-urlencode content@src/candid/token.did -u ${BASIC_AUTH_USERNAME}:${BASIC_AUTH_PASSWORD}
