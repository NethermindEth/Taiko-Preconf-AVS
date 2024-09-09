curl -X POST http://localhost:8080/eth/v1/builder/constraints -H "Content-Type: application/json" -d '[{
    "message" : {
        "validator_index" : 12345,
        "slot" : 8978583,
        "constraints" : [
            {
                "tx" : "0x02f871018304a5758085025ff11caf82565f94388c818ca8b9251b393131c08a736a67ccb1929787a41bb7ee22b41380c001a0c8630f734aba7acb4275a8f3b0ce831cf0c7c487fd49ee7bcca26ac622a28939a04c3745096fa0130a188fa249289fd9e60f9d6360854820dba22ae779ea6f573f",
                "index" : 0
            }
        ]
    },
    "signature" : "0x81510b571e22f89d1697545aac01c9ad0c1e7a3e778b3078bef524efae14990e58a6e960a152abd49de2e18d7fd3081c15d5c25867ccfad3d47beef6b39ac24b6b9fbf2cfa91c88f67aff750438a6841ec9e4a06a94ae41410c4f97b75ab284c"
    }]'