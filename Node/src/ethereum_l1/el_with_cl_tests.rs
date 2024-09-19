#[cfg(test)]
mod tests {
    use super::super::{
        consensus_layer::tests::setup_server, consensus_layer::ConsensusLayer,
        execution_layer::ExecutionLayer, execution_layer::PreconfTaskManager,
    };
    use alloy::node_bindings::Anvil;

    #[tokio::test]
    async fn test_propose_new_block_with_lookahead() {
        let server = setup_server().await;
        let cl = ConsensusLayer::new(server.url().as_str()).unwrap();
        let _duties = cl.get_lookahead(1).await.unwrap();

        let anvil = Anvil::new().try_spawn().unwrap();
        let rpc_url: reqwest::Url = anvil.endpoint().parse().unwrap();
        let ws_rpc_url = anvil.ws_endpoint();
        let private_key = anvil.keys()[0].clone();
        let el = ExecutionLayer::new_from_pk(ws_rpc_url, rpc_url, private_key)
            .await
            .unwrap();

        // TODO:
        // There is a bug in the Anvil (anvil 0.2.0) library:
        // `Result::unwrap()` on an `Err` value: buffer overrun while deserializing
        // check if it's fixed in next version
        // let lookahead_params = el
        //     .get_lookahead_params_for_epoch_using_cl_lookahead(1, &duties)
        //     .await
        //     .unwrap();
        let lookahead_params = Vec::<PreconfTaskManager::LookaheadSetParam>::new();

        el.propose_new_block(0, vec![0; 32], [0; 32], 0, lookahead_params, true)
            .await
            .unwrap();
    }
}
