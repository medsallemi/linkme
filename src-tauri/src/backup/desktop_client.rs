use std::{fs::File, io::Read, path::Path};

use serde::{Deserialize, Serialize};

const MAX_CONFIG: usize = 32 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DesktopExport {
    installed: Client,
}

#[derive(Deserialize, Serialize)]
pub(super) struct Client {
    pub(super) client_id: String,
    pub(super) client_secret: String,
}

impl Client {
    pub(super) fn from_export(path: &Path) -> Option<Self> {
        let mut bytes = Vec::new();
        File::open(path)
            .ok()?
            .take((MAX_CONFIG + 1) as u64)
            .read_to_end(&mut bytes)
            .ok()?;
        if bytes.len() > MAX_CONFIG {
            return None;
        }
        let DesktopExport { installed } = serde_json::from_slice(&bytes).ok()?;
        installed.valid().then_some(installed)
    }

    pub(super) fn valid(&self) -> bool {
        fn google_value(value: &str) -> bool {
            !value.is_empty()
                && value.len() <= 200
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        }
        self.client_id
            .strip_suffix(".apps.googleusercontent.com")
            .is_some_and(google_value)
            && google_value(&self.client_secret)
    }
}
