use alloy::sol;

sol!(
    #[allow(missing_docs)]
    #[sol(rpc)]
    TaikoAnchor,
    "src/taiko/abi/TaikoAnchor.json"
);
