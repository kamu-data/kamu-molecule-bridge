pub mod prelude;

use alloy::sol;

sol!(
    #[sol(rpc)]
    IPNFT,
    "abis/IPNFT.json"
);
sol!(
    #[sol(rpc)]
    IPToken,
    "abis/IPToken.json"
);
sol!(
    #[sol(rpc)]
    Safe,
    "abis/Safe.json"
);
sol!(
    #[sol(rpc)]
    Tokenizer,
    "abis/Tokenizer.json"
);
// NOTE: Backward compatibility, based on:
//       https://github.com/moleculeprotocol/IPNFT/blob/main/subgraph/makeAbis.sh
sol!(
    #[sol(rpc)]
    Synthesizer,
    "abis/Synthesizer.json"
);
