//! Wire models keep byte counts as strings to preserve u64 precision across JavaScript.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Device {
    pub device_id: String,
    pub display_name: String,
    pub os: String,
    pub address: String,
    #[serde(default)]
    pub online: bool,
    #[serde(default)]
    pub paired: bool,
    #[serde(default)]
    pub certificate: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Item {
    pub name: String,
    pub size: String,
    /// Decimal bytes keep progress exact beyond JavaScript's integer precision limit.
    #[serde(default = "zero_bytes")]
    pub bytes_transferred: String,
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub saved_name: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Transfer {
    pub transfer_id: String,
    pub peer_id: String,
    pub direction: String,
    pub kind: String,
    pub state: String,
    pub created_at: u64,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub items: Vec<Item>,
    #[serde(default)]
    pub error: Option<String>,
}
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

// Old history and older peers start at zero rather than exposing an invalid empty byte count.
fn zero_bytes() -> String {
    "0".into()
}
