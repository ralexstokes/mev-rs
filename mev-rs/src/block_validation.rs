use std::cmp::Ordering;

pub const GAS_BOUND_DIVISOR: u64 = 1024;

pub fn compute_preferred_gas_limit(preferred_gas_limit: u64, parent_gas_limit: u64) -> u64 {
    match preferred_gas_limit.cmp(&parent_gas_limit) {
        Ordering::Equal => preferred_gas_limit,
        Ordering::Greater => {
            let bound = parent_gas_limit + parent_gas_limit / GAS_BOUND_DIVISOR;
            preferred_gas_limit.min(bound - 1)
        }
        Ordering::Less => {
            let bound = parent_gas_limit - parent_gas_limit / GAS_BOUND_DIVISOR;
            preferred_gas_limit.max(bound + 1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn verify_limits(gas_limit: u64, parent_gas_limit: u64) -> bool {
        match gas_limit.cmp(&parent_gas_limit) {
            Ordering::Equal => true,
            Ordering::Greater => {
                let bound = parent_gas_limit + parent_gas_limit / GAS_BOUND_DIVISOR;
                gas_limit < bound
            }
            Ordering::Less => {
                let bound = parent_gas_limit - parent_gas_limit / GAS_BOUND_DIVISOR;
                gas_limit > bound
            }
        }
    }

    #[test]
    fn test_compute_preferred_gas_limit() {
        for t in &[
            // preferred, parent, computed
            (30_000_000, 30_000_000, 30_000_000),
            (30_029_000, 30_000_000, 30_029_000),
            (30_029_300, 30_000_000, 30_029_295),
            (29_970_710, 30_000_000, 29_970_710),
            (29_970_700, 30_000_000, 29_970_705),
        ] {
            assert_eq!(compute_preferred_gas_limit(t.0, t.1), t.2);
            assert!(verify_limits(t.2, t.1))
        }
    }
}
