use std::{collections::BTreeMap, time::Duration};

use serde::{Deserialize, Serialize};

use crate::{BaselineError, MetricsSummary, Result};

/// Opt-in regression thresholds for a finite load test, not production launch criteria.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptanceCriteria {
    /// Minimum confirmed execution gas per second.
    pub min_gps: u64,
    /// Minimum canonical blocks observed during measured submission.
    pub min_blocks: u64,
    /// Maximum p95 submission-to-canonical-inclusion latency in milliseconds.
    pub max_block_latency_p95_ms: u64,
    /// Maximum p95 canonical RPC availability lag in milliseconds.
    pub max_availability_lag_p95_ms: u64,
}

impl AcceptanceCriteria {
    /// Rejects empty thresholds rather than silently weakening an acceptance run.
    pub fn validate(&self) -> Result<()> {
        if self.min_gps == 0
            || self.min_blocks == 0
            || self.max_block_latency_p95_ms == 0
            || self.max_availability_lag_p95_ms == 0
        {
            return Err(BaselineError::Config("acceptance thresholds must be > 0".into()));
        }
        Ok(())
    }

    /// Evaluates thresholds and mandatory data-quality checks after receipt enrichment.
    ///
    /// Empty, failed, partially confirmed, or incomplete-receipt runs cannot pass, even
    /// when their default latency metrics are zero. This does not validate fork activation
    /// or consensus timestamp correctness; those require protocol integration tests.
    pub fn evaluate(&self, summary: &MetricsSummary) -> AcceptanceReport {
        let throughput = &summary.throughput;
        let receipts = &summary.receipt_coverage;
        let checks = BTreeMap::from([
            ("no_run_error".into(), summary.error.is_none()),
            (
                "confirmations_complete".into(),
                throughput.total_confirmed > 0
                    && throughput.total_confirmed == throughput.total_submitted
                    && summary.pacing.undrained_transactions == 0,
            ),
            (
                "receipts_complete".into(),
                receipts.is_complete()
                    && receipts.transactions_matched == throughput.total_confirmed
                    && receipts.transactions_total == throughput.total_confirmed
                    && receipts.blocks_total > 0,
            ),
            ("no_submission_failures".into(), throughput.total_failed == 0),
            ("no_reverts".into(), throughput.total_reverted == 0),
            ("min_gps".into(), throughput.gps.is_finite() && throughput.gps >= self.min_gps as f64),
            ("min_blocks".into(), summary.pacing.blocks_observed >= self.min_blocks),
            (
                "max_block_latency_p95_ms".into(),
                summary.block_latency.p95 <= Duration::from_millis(self.max_block_latency_p95_ms),
            ),
            (
                "max_availability_lag_p95_ms".into(),
                summary.pacing.availability_lag.p95
                    <= Duration::from_millis(self.max_availability_lag_p95_ms),
            ),
        ]);
        AcceptanceReport {
            passed: checks.values().all(|passed| *passed),
            criteria: self.clone(),
            checks,
        }
    }
}

/// Machine-readable acceptance decision included in the ordinary metrics JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcceptanceReport {
    /// Whether every check passed.
    pub passed: bool,
    /// Thresholds used for this decision.
    pub criteria: AcceptanceCriteria,
    /// Stable check names mapped to pass/fail decisions.
    pub checks: BTreeMap<String, bool>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ReceiptCoverage;

    #[test]
    fn acceptance_fails_closed_and_reports_individual_threshold_failures() {
        let criteria = AcceptanceCriteria {
            min_gps: 18_000_000,
            min_blocks: 125,
            max_block_latency_p95_ms: 1_000,
            max_availability_lag_p95_ms: 100,
        };
        let mut summary = MetricsSummary::default();
        assert!(!criteria.evaluate(&summary).passed);

        summary.throughput.total_submitted = 100;
        summary.throughput.total_confirmed = 100;
        summary.throughput.gps = 18_000_000.0;
        summary.pacing.blocks_observed = 125;
        summary.block_latency.p95 = Duration::from_secs(1);
        summary.pacing.availability_lag.p95 = Duration::from_millis(100);
        summary.receipt_coverage = ReceiptCoverage {
            blocks_total: 125,
            transactions_total: 100,
            transactions_matched: 100,
            ..Default::default()
        };
        assert!(criteria.evaluate(&summary).passed);

        summary.throughput.gps = f64::NAN;
        summary.block_latency.p95 += Duration::from_nanos(1);
        summary.pacing.availability_lag.p95 += Duration::from_nanos(1);
        summary.pacing.blocks_observed -= 1;
        summary.receipt_coverage.transactions_missing = 1;
        summary.throughput.total_failed = 1;
        summary.throughput.total_reverted = 1;
        summary.throughput.total_confirmed -= 1;
        summary.error = Some("interrupted".into());
        let report = criteria.evaluate(&summary);
        assert!(!report.passed);
        assert!(report.checks.values().all(|passed| !passed));
        let json = serde_json::to_value(report).unwrap();
        assert_eq!(json["passed"], false);
        assert_eq!(json["criteria"]["min_gps"], 18_000_000);
        assert_eq!(json["checks"]["receipts_complete"], false);
    }
}
