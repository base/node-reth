//! Expected chain identity selected by the unified CLI.

/// L1 and L2 chain IDs expected from the configured RPC endpoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BatcherChainIds {
    /// Expected L1 chain ID.
    pub l1: u64,
    /// Expected L2 chain ID.
    pub l2: u64,
}

impl BatcherChainIds {
    /// Rejects RPC endpoints that belong to a different network.
    pub fn validate(self, l1: u64, l2: u64) -> eyre::Result<()> {
        eyre::ensure!(self.l1 == l1, "L1 chain ID mismatch: expected {}, got {l1}", self.l1);
        eyre::ensure!(self.l2 == l2, "L2 chain ID mismatch: expected {}, got {l2}", self.l2);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::BatcherChainIds;

    #[test]
    fn accepts_matching_chain_ids() {
        BatcherChainIds { l1: 1, l2: 8453 }.validate(1, 8453).unwrap();
    }

    #[test]
    fn rejects_wrong_l1_or_l2() {
        let expected = BatcherChainIds { l1: 1, l2: 8453 };
        assert!(
            expected
                .validate(11155111, 8453)
                .unwrap_err()
                .to_string()
                .contains("L1 chain ID mismatch")
        );
        assert!(
            expected.validate(1, 84532).unwrap_err().to_string().contains("L2 chain ID mismatch")
        );
    }
}
