//! Azure Speech REST client (SSML in, mp3 out). Never logs the key.
use crate::AzureCreds;
use std::time::Duration;

pub const BASE_URL_ENV: &str = "NYX_AZURE_SPEECH_BASE_URL";

pub fn base_url(creds: &AzureCreds, explicit: Option<&str>) -> String {
    if let Some(b) = explicit.filter(|b| !b.is_empty()) {
        return b.trim_end_matches('/').to_string();
    }
    if let Ok(b) = std::env::var(BASE_URL_ENV) {
        if !b.trim().is_empty() {
            return b.trim().trim_end_matches('/').to_string();
        }
    }
    format!("https://{}.tts.speech.microsoft.com", creds.region)
}

pub fn request(base: &str, creds: &AzureCreds, ssml: &str, timeout: Duration) -> anyhow::Result<Vec<u8>> {
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(timeout)).build().into();
    let url = format!("{}/cognitiveservices/v1", base);
    let res = agent
        .post(&url)
        .header("Ocp-Apim-Subscription-Key", &creds.key)
        .header("Content-Type", "application/ssml+xml")
        .header("X-Microsoft-OutputFormat", "audio-24khz-48kbitrate-mono-mp3")
        .header("User-Agent", "nyx")
        .send(ssml.as_bytes());
    match res {
        Ok(mut r) => {
            let bytes = r.body_mut().read_to_vec().map_err(|e| anyhow::anyhow!("azure body read failed: {}", e))?;
            if bytes.is_empty() {
                anyhow::bail!("azure returned an empty body");
            }
            Ok(bytes)
        }
        Err(ureq::Error::StatusCode(code)) => anyhow::bail!("azure HTTP {}", code),
        Err(e) => anyhow::bail!("azure network error: {}", e),
    }
}
