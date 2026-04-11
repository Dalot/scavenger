use crate::eval::CaseResult;
use crate::eval::corpus::CorpusEntry;
use crate::eval::thresholds::Thresholds;

pub fn run_accuracy_eval(
    _corpus: &[CorpusEntry],
    _thresholds: &Thresholds,
) -> Result<Vec<CaseResult>, String> {
    Ok(Vec::new())
}
