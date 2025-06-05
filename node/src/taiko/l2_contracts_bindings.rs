use alloy::sol;

sol!(
    #[allow(missing_docs)]
    #[sol(rpc)]
    TaikoAnchor,
    "src/taiko/abi/TaikoAnchor.json"
);

pub mod bridge {
    use super::*;

    sol!(
        #[allow(missing_docs)]
        #[sol(rpc)]
        IBridge,
        "src/taiko/abi/IBridge.json"
    );
}
