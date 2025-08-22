use alloy::sol;

sol!(
    #[allow(missing_docs)]
    #[sol(rpc)]
    Urc,
    "src/l1/abi/IRegistry.json"
);
