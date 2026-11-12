//! Validation of a storage tree before any write (FR-014). T070 replaces this stub with the
//! rules in data-model.md; until then `check` calls it and gets `Ok`.

use super::tree::Node;

/// Every FR-014 rule a tree breaks, or `Ok`. A stub until T070.
pub fn validate(_nodes: &[Node]) -> Result<(), Vec<String>> {
    Ok(())
}
