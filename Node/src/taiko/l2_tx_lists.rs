use anyhow::Error;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct RPCReplyL2TxLists {
    pub tx_lists: Value, // TODO: decode and create tx_list_bytes on AVS node side
    #[serde(deserialize_with = "deserialize_tx_lists_bytes")]
    pub tx_list_bytes: Vec<Vec<u8>>,
    #[serde(deserialize_with = "deserialize_parent_meta_hash")]
    pub parent_meta_hash: [u8; 32],
}

fn deserialize_tx_lists_bytes<'de, D>(deserializer: D) -> Result<Vec<Vec<u8>>, D::Error>
where
    D: Deserializer<'de>,
{
    let vec: Vec<String> = Deserialize::deserialize(deserializer)?;
    let result = vec
        .iter()
        .map(|s| s.as_bytes().to_vec())
        .collect::<Vec<Vec<u8>>>();
    Ok(result)
}

fn deserialize_parent_meta_hash<'de, D>(deserializer: D) -> Result<[u8; 32], D::Error>
where
    D: Deserializer<'de>,
{
    let s: String = Deserialize::deserialize(deserializer)?;
    let s = s.trim_start_matches("0x");
    let bytes = hex::decode(s).map_err(serde::de::Error::custom)?;
    if bytes.len() != 32 {
        return Err(serde::de::Error::custom(
            "Invalid length for parent_meta_hash",
        ));
    }
    let mut array = [0u8; 32];
    array.copy_from_slice(&bytes);
    Ok(array)
}

pub fn decompose_pending_lists_json(json: Value) -> Result<RPCReplyL2TxLists, Error> {
    // Deserialize the JSON string into the struct
    let rpc_reply: RPCReplyL2TxLists = serde_json::from_value(json)?;
    Ok(rpc_reply)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decompose_pending_lists_json() {
        let json_data = serde_json::json!(
            {
                "TxLists": [
                    [
                        {
                            "type": "0x0",
                            "chainId": "0x28c61",
                            "nonce": "0x8836",
                            "to": "0x8b14d287b4150ff22ac73df8be720e933f659abc",
                            "gas": "0x35f30",
                            "gasPrice": "0xf4b87001",
                            "maxPriorityFeePerGas": null,
                            "maxFeePerGas": null,
                            "value": "0x0",
                            "input": "0x3161b7f60000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000000000000100000000000000000000000000000000000000000000000000000000000027e9000000000000000000000000000000000000000000000005751b6fc9babade290000000000000000000000000000000000000000000000000000000008f811330000000000000000000000000000000000000000000000000000000000000010",
                            "v": "0x518e5",
                            "r": "0xb7b4b5540e08775ebb3c077ca7d572378cdb6ed55e3387173f8248578cc073e9",
                            "s": "0x1f8860f90b61202d4070d1eba494d38c2cb02c749ea1ec542c8d02e5ceeb6439",
                            "hash": "0xc653e446eafe51eea1f46e6e351adbd1cc8a3271e6935f1441f613a58d441f6a"
                        },
                        {
                            "type": "0x2",
                            "chainId": "0x28c61",
                            "nonce": "0x26d0",
                            "to": "0x2f6ef5baae08ae9df23528c217a77f03f93b690e",
                            "gas": "0x1a09b",
                            "gasPrice": null,
                            "maxPriorityFeePerGas": "0xcbef0801",
                            "maxFeePerGas": "0xcbef0801",
                            "value": "0x0",
                            "input": "0xbb2cd728000000000000000000000000000000000000000000000000000000000000004000000000000000000000000000000000000000000000000000000000000000a00000000000000000000000000000000000000000000000000000000000000002000000000000000000000000ae2c46ddb314b9ba743c6dee4878f151881333d90000000000000000000000007d16e966c879ed6ad9972ddd5376c41a427d2c2a00000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000004cd712049c0000000000000000000000000000000000000000000000000000057b3ac2af84",
                            "accessList": [],
                            "v": "0x0",
                            "r": "0xbfc407896ee6ed36962d1a591b41aa9a17fe19e176e61cb54a1cd69e3a337508",
                            "s": "0x621b2448dda4d797447935c051e3908f65dfb3729415280a04cb02c4474baefe",
                            "yParity": "0x0",
                            "hash": "0xffbcd2fab90f1bf314ca2da1bf83eeab3d17fd58a0393d29a697b2ff05d0e65c"
                        }
                    ]
                ],
                "TxListBytes": [
                    "eJz6ybTuJ6NeU4dZy5cdBYzNzPEGU7pFLrVvEeX/pHXc9se+Ir7J9qmz9jTsOGKYuP0bA36gQECeEb+0+kscEqyl0vknd+26p4lPN8cPQWMC9gs0s0o8XbB9y9YQPo7yuN027DXLrxaZ99zOuxpn3C5u3+QR3nOg+OUC+Y6En9yJCroOBRdfL5lyuUdng07JvIVvQnR6mZ6ee51iuZOxiuknY1kzU09ik9qFltPvORjBRDPjgtlT9PO+7lrHsW7uJ1ONQ+LL65l/WmfyNexkZNmtc12DgPMcCMgvICDPhMxZp+N2d7PIzl0lNrnvPCo+BnYIG99Elq8Ve5l2ovJt1s3puneDy45IOdXqaJFiPhrwuS7EMge3NGu11aH1LQcaFuw/wt6Z9+yt2TRdqUhpx1WzxP9JPix7JrPVS+baPCvjUo4FSdIqHneXXJ/uUml6IPDxhP7U+5uLpohqcLGcZjri7r3uHyAAAP//huiQHQ=="
                ],
                "ParentMetaHash": "0x2bcf3b1bb0c4066aa46ba6e99b79d7602f124d5ae8fcffd2977b1c2138aa61bc"
            }
        );

        let result = decompose_pending_lists_json(json_data).unwrap();

        assert_eq!(result.tx_lists.as_array().unwrap().len(), 1);
        assert_eq!(
            result.tx_lists.as_array().unwrap()[0]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(result.tx_list_bytes.len(), 1);
        assert_eq!(result.tx_list_bytes[0].len(), 492);
        assert_eq!(result.parent_meta_hash.len(), 32);
    }
}
