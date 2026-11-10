//! Windows fallback voice (Windows.Media.SpeechSynthesis, "Microsoft Maria").
use crate::VoiceParams;

#[cfg(windows)]
mod imp {
    use super::VoiceParams;
    use windows::Media::SpeechSynthesis::{SpeechSynthesizer, VoiceGender, VoiceInformation};
    use windows::Storage::Streams::DataReader;
    use windows::core::HSTRING;

    pub fn pick_voice() -> anyhow::Result<VoiceInformation> {
        let all = SpeechSynthesizer::AllVoices()?;
        for v in all {
            let lang = v.Language()?.to_string();
            if lang.eq_ignore_ascii_case("pt-BR") && v.Gender()? == VoiceGender::Female {
                return Ok(v);
            }
        }
        anyhow::bail!("no pt-BR female Windows voice installed")
    }

    pub fn voice_name() -> Option<String> {
        pick_voice().ok().and_then(|v| v.DisplayName().ok()).map(|n| n.to_string())
    }

    pub fn synth_wav(text: &str, p: &VoiceParams) -> anyhow::Result<Vec<u8>> {
        let voice = pick_voice()?;
        let synth = SpeechSynthesizer::new()?;
        synth.SetVoice(&voice)?;
        if let Ok(opts) = synth.Options() {
            let _ = opts.SetSpeakingRate((1.0 + p.rate_pct as f64 / 100.0).clamp(0.5, 2.0));
            let _ = opts.SetAudioPitch((1.0 + p.pitch_pct as f64 / 100.0).clamp(0.0, 2.0));
        }
        let stream = synth.SynthesizeTextToStreamAsync(&HSTRING::from(text))?.get()?;
        let size = stream.Size()? as u32;
        let reader = DataReader::CreateDataReader(&stream.GetInputStreamAt(0)?)?;
        reader.LoadAsync(size)?.get()?;
        let mut buf = vec![0u8; size as usize];
        reader.ReadBytes(&mut buf)?;
        Ok(buf)
    }
}

#[cfg(windows)]
pub fn voice_name() -> Option<String> {
    imp::voice_name()
}
#[cfg(windows)]
pub fn synth_wav(text: &str, p: &VoiceParams) -> anyhow::Result<Vec<u8>> {
    imp::synth_wav(text, p)
}
#[cfg(not(windows))]
pub fn voice_name() -> Option<String> {
    None
}
#[cfg(not(windows))]
pub fn synth_wav(_text: &str, _p: &VoiceParams) -> anyhow::Result<Vec<u8>> {
    anyhow::bail!("Windows speech synthesis is only available on Windows")
}
