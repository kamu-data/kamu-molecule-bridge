use alloy::primitives::Address;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct OclOwnershipProjection {
    pub current: Option<Address>,
    pub previous: Vec<Address>,
}

impl OclOwnershipProjection {
    pub fn new(initial_owner: Address) -> Self {
        Self {
            current: Some(initial_owner),
            previous: vec![],
        }
    }

    // TODO: verify from/previous owner?
    pub fn apply_transfer(&mut self, new_owner: Address) -> Option<Address> {
        if self.current == Some(new_owner) {
            return None;
        }

        self.previous.retain(|previous| *previous != new_owner);

        let previous_owner = self.current;

        if let Some(previous_owner) = previous_owner {
            self.previous.push(previous_owner);
        }

        self.current = Some(new_owner);

        previous_owner
    }
}
