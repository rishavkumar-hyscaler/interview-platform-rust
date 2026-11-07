use serde_json::Value;
pub(crate) struct AssessmentReport {
    pub state: String,
    pub assessment: Option<Value>,
    pub attempts: i64,
    pub last_failure: Option<Value>,
    pub observations: Vec<Value>,
}
