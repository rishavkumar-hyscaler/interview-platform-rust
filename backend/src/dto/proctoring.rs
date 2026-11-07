//! Browser observation input contract, separate from persisted event values.
use serde::Deserialize;
use uuid::Uuid;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ObservationRequest {
    pub id: Uuid,
    pub kind: String,
    pub client_time: i64,
    pub count: Option<i32>,
    pub measurements: Option<MeasurementsRequest>,
}
#[derive(Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MeasurementsRequest {
    pub duration_ms: u32,
    pub face_count: Option<u8>,
    pub speech_probability: Option<f32>,
    pub horizontal_offset: Option<f32>,
    pub vertical_offset: Option<f32>,
    pub mouth_motion: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub head_horizontal_offset: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub head_vertical_offset: Option<f32>,
}

#[derive(serde::Serialize)]
pub(crate) struct AcceptedResponse {
    pub accepted: bool,
}
