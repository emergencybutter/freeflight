use serde::{Deserialize, Serialize};

/// A weight at a station (distance from datum, in inches), used both for
/// the aircraft's fixed items (empty weight) and loaded items (pax,
/// bags, fuel).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WeightAtStation {
    pub weight_lb: f64,
    pub arm_in: f64,
}

impl WeightAtStation {
    pub fn moment(&self) -> f64 {
        self.weight_lb * self.arm_in
    }
}

/// A single-envelope, forward/aft-limit weight & balance check
/// (DESIGN.md §9.3 — explicitly *not* a certified multi-envelope tool).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WeightBalanceEnvelope {
    pub max_gross_weight_lb: f64,
    pub forward_cg_limit_in: f64,
    pub aft_cg_limit_in: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WeightBalanceResult {
    pub total_weight_lb: f64,
    pub cg_in: f64,
    pub within_weight_limit: bool,
    pub within_cg_limits: bool,
}

impl WeightBalanceResult {
    pub fn is_within_limits(&self) -> bool {
        self.within_weight_limit && self.within_cg_limits
    }
}

/// Sum weight/moment across all loaded stations and check against a
/// single envelope.
pub fn check(items: &[WeightAtStation], envelope: &WeightBalanceEnvelope) -> WeightBalanceResult {
    let total_weight_lb: f64 = items.iter().map(|i| i.weight_lb).sum();
    let total_moment: f64 = items.iter().map(|i| i.moment()).sum();
    let cg_in = if total_weight_lb > 0.0 {
        total_moment / total_weight_lb
    } else {
        0.0
    };

    WeightBalanceResult {
        total_weight_lb,
        cg_in,
        within_weight_limit: total_weight_lb <= envelope.max_gross_weight_lb,
        within_cg_limits: cg_in >= envelope.forward_cg_limit_in
            && cg_in <= envelope.aft_cg_limit_in,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_an_overweight_loading() {
        let envelope = WeightBalanceEnvelope {
            max_gross_weight_lb: 2450.0,
            forward_cg_limit_in: 35.0,
            aft_cg_limit_in: 47.3,
        };
        let items = [
            WeightAtStation {
                weight_lb: 1600.0,
                arm_in: 39.0,
            }, // empty
            WeightAtStation {
                weight_lb: 400.0,
                arm_in: 37.0,
            }, // pilot + pax
            WeightAtStation {
                weight_lb: 500.0,
                arm_in: 48.0,
            }, // fuel + bags
        ];
        let result = check(&items, &envelope);
        assert!(!result.within_weight_limit);
        assert_eq!(result.total_weight_lb, 2500.0);
    }

    #[test]
    fn accepts_a_loading_within_the_envelope() {
        let envelope = WeightBalanceEnvelope {
            max_gross_weight_lb: 2450.0,
            forward_cg_limit_in: 35.0,
            aft_cg_limit_in: 47.3,
        };
        let items = [
            WeightAtStation {
                weight_lb: 1600.0,
                arm_in: 39.0,
            },
            WeightAtStation {
                weight_lb: 340.0,
                arm_in: 37.0,
            },
            WeightAtStation {
                weight_lb: 300.0,
                arm_in: 48.0,
            },
        ];
        let result = check(&items, &envelope);
        assert!(result.is_within_limits(), "result was {result:?}");
    }
}
