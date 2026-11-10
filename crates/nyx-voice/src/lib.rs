//! Nyx voice (TTS). Spec: docs/HANDOFF.md "TTS". Owner: agent B.
//! The public signatures below are the contract the core (agent A) compiles against;
//! agent B replaces the stub bodies and may add modules, but must not change these
//! signatures without the orchestrator.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

mod azure;
mod cache;
mod envelope;
mod normalize;
mod winvoice;

#[cfg(test)]
mod tests;

const AZURE_TIMEOUT: Duration = Duration::from_secs(10);

pub const DEFAULT_VOICE: &str = "pt-BR-FranciscaNeural";
pub const FREE_TIER_CHARS: u64 = 500_000;
pub const KEY_ENV: &str = "NYX_AZURE_SPEECH_KEY";
pub const REGION_ENV: &str = "NYX_AZURE_SPEECH_REGION";
/// One envelope value per this many milliseconds.
pub const ENVELOPE_STEP_MS: u32 = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Engine {
    Azure,
    Windows,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EnginePref {
    Auto,
    Azure,
    Windows,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioFormat {
    Mp3,
    Wav,
}

/// Synthesized audio, ready to be decoded/played by the core.
#[derive(Debug, Clone)]
pub struct Audio {
    pub engine: Engine,
    pub format: AudioFormat,
    pub bytes: Vec<u8>,
    /// True when served from the disk cache (no Azure characters spent).
    pub cached: bool,
}

#[derive(Debug, Clone)]
pub struct VoiceParams {
    pub voice_name: String,
    /// Percent offsets for SSML prosody (e.g. -5 = 5% slower / lower).
    pub rate_pct: i32,
    pub pitch_pct: i32,
}

impl Default for VoiceParams {
    fn default() -> Self {
        Self { voice_name: DEFAULT_VOICE.to_string(), rate_pct: -5, pitch_pct: -5 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AzureCreds {
    pub key: String,
    pub region: String,
}

/// `NYX_AZURE_SPEECH_KEY` + `NYX_AZURE_SPEECH_REGION`, both non-empty.
pub fn azure_creds_from_env() -> Option<AzureCreds> {
    let key = std::env::var(KEY_ENV).ok()?.trim().to_string();
    let region = std::env::var(REGION_ENV).ok()?.trim().to_string();
    if key.is_empty() || region.is_empty() {
        return None;
    }
    Some(AzureCreds { key, region })
}

/// Text cleanup for speech (pt-BR): "Dev", file:line refs, acronyms, US$, emoji/markdown removal.
pub fn normalize_for_speech(text: &str) -> String {
    normalize::normalize(text)
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

/// SSML document for Azure with prosody (rate/pitch) and pt-BR lang.
pub fn build_ssml(text: &str, p: &VoiceParams) -> String {
    format!(
        "<speak version=\"1.0\" xml:lang=\"pt-BR\"><voice name=\"{}\"><prosody rate=\"{:+}%\" pitch=\"{:+}%\">{}</prosody></voice></speak>",
        xml_escape(&p.voice_name),
        p.rate_pct,
        p.pitch_pct,
        xml_escape(text)
    )
}

/// Windows backend abstraction so tests never touch the real speech engine.
pub(crate) trait WinBackend {
    fn voice(&self) -> String;
    fn synth(&self, text: &str, p: &VoiceParams) -> anyhow::Result<Vec<u8>>;
}

struct RealWin;
impl WinBackend for RealWin {
    fn voice(&self) -> String {
        windows_voice_name().unwrap_or_default()
    }
    fn synth(&self, text: &str, p: &VoiceParams) -> anyhow::Result<Vec<u8>> {
        winvoice::synth_wav(text, p)
    }
}

/// Synthesizer with disk cache (LRU, `cache_dir`, max bytes) and monthly Azure character counter.
pub struct Synth {
    pub cache_dir: PathBuf,
    pub max_cache_bytes: u64,
    /// Override for tests (`NYX_AZURE_SPEECH_BASE_URL` or explicit); default `https://{region}.tts.speech.microsoft.com`.
    pub azure_base_url: Option<String>,
}

impl Synth {
    pub fn new(cache_dir: &Path) -> Self {
        Self { cache_dir: cache_dir.to_path_buf(), max_cache_bytes: 50 * 1024 * 1024, azure_base_url: None }
    }

    /// Picks the engine (`Auto` = Azure when creds present and the call succeeds, else Windows),
    /// normalizes the text, serves from cache when possible and counts Azure characters.
    pub fn synthesize(&self, text: &str, p: &VoiceParams, pref: EnginePref, creds: Option<&AzureCreds>) -> anyhow::Result<Audio> {
        self.synthesize_inner(text, p, pref, creds, AZURE_TIMEOUT, &RealWin)
    }

    pub(crate) fn synthesize_inner(
        &self,
        text: &str,
        p: &VoiceParams,
        pref: EnginePref,
        creds: Option<&AzureCreds>,
        timeout: Duration,
        win: &dyn WinBackend,
    ) -> anyhow::Result<Audio> {
        let norm = normalize_for_speech(text);
        if norm.is_empty() {
            anyhow::bail!("nothing to speak after normalization");
        }
        let use_azure = match pref {
            EnginePref::Windows => false,
            EnginePref::Azure => {
                if creds.is_none() {
                    anyhow::bail!("Azure engine requested but no credentials are configured");
                }
                true
            }
            EnginePref::Auto => creds.is_some(),
        };
        if let (true, Some(c)) = (use_azure, creds) {
            let key = cache::cache_key(Engine::Azure, &p.voice_name, p, &norm);
            if let Some(bytes) = cache::get(&self.cache_dir, &key, AudioFormat::Mp3) {
                return Ok(Audio { engine: Engine::Azure, format: AudioFormat::Mp3, bytes, cached: true });
            }
            let base = azure::base_url(c, self.azure_base_url.as_deref());
            match azure::request(&base, c, &build_ssml(&norm, p), timeout) {
                Ok(bytes) => {
                    cache::add_chars(&self.cache_dir, norm.chars().count() as u64);
                    cache::put(&self.cache_dir, &key, AudioFormat::Mp3, &bytes, self.max_cache_bytes);
                    return Ok(Audio { engine: Engine::Azure, format: AudioFormat::Mp3, bytes, cached: false });
                }
                Err(e) if pref == EnginePref::Azure => return Err(e),
                Err(e) => eprintln!("[nyx-voice] Azure failed, falling back to Windows voice: {e}"),
            }
        }
        let key = cache::cache_key(Engine::Windows, &win.voice(), p, &norm);
        if let Some(bytes) = cache::get(&self.cache_dir, &key, AudioFormat::Wav) {
            return Ok(Audio { engine: Engine::Windows, format: AudioFormat::Wav, bytes, cached: true });
        }
        let bytes = win.synth(&norm, p)?;
        cache::put(&self.cache_dir, &key, AudioFormat::Wav, &bytes, self.max_cache_bytes);
        Ok(Audio { engine: Engine::Windows, format: AudioFormat::Wav, bytes, cached: false })
    }

    /// Azure characters synthesized in the current calendar month (persisted in `cache_dir`).
    pub fn month_chars(&self) -> u64 {
        cache::month_chars(&self.cache_dir)
    }
}

/// Name of the installed Windows pt-BR female voice (e.g. "Microsoft Maria"), if any.
pub fn windows_voice_name() -> Option<String> {
    winvoice::voice_name()
}

/// Mouth envelope from mono PCM: one value in 0..1 per `ENVELOPE_STEP_MS`, smoothed, speech-gated.
pub fn envelope_from_pcm(samples: &[f32], sample_rate: u32) -> Vec<f32> {
    envelope::envelope(samples, sample_rate)
}
