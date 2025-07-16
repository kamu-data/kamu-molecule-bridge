use alloy::primitives::B256;
use alloy::rpc::types::Log;

pub trait LogExt {
    fn event_signature_hash(&self) -> B256;
}

impl LogExt for Log {
    #[inline]
    fn event_signature_hash(&self) -> B256 {
        // SAFETY: The first topic is always the event signature hash
        self.topics()[0]
    }
}
