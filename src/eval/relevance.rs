use crate::eval::CaseResult;
use crate::eval::corpus::CorpusEntry;
use crate::eval::thresholds::Thresholds;

pub fn run_relevance_eval(
    _corpus: &[CorpusEntry],
    _thresholds: &Thresholds,
) -> Result<Vec<CaseResult>, String> {
    Ok(Vec::new())
}

pub fn run_performance_checks(
    _corpus: &[CorpusEntry],
    _thresholds: &Thresholds,
) -> Result<Vec<CaseResult>, String> {
    Ok(Vec::new())
}
