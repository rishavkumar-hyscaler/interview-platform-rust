//! Verified binary asset data. HTTP headers are assembled by the controller.
pub(crate) struct Asset {
    pub bytes: Vec<u8>,
    pub mime: String,
    pub etag: String,
}
