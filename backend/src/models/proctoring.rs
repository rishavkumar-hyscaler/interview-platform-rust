//! Internal validated observation values used by the event service.
use serde::Deserialize;
use uuid::Uuid;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Observation {
    pub id: Uuid,
    pub kind: String,
    pub client_time: i64,
    pub count: Option<i32>,
    pub measurements: Option<Measurements>,
}
#[derive(Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Measurements {
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
