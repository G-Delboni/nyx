//! `cargo run -p nyx-voice --example speak -- "texto"` writes speak.wav using the Windows voice.
use nyx_voice::{EnginePref, Synth, VoiceParams};

fn main() -> anyhow::Result<()> {
    let text = std::env::args().skip(1).collect::<Vec<_>>().join(" ");
    let text = if text.is_empty() { "Dev terminou a etapa. Posso seguir para a próxima?".to_string() } else { text };
    println!("voz: {:?}", nyx_voice::windows_voice_name());
    let synth = Synth::new(&std::env::temp_dir().join("nyx-voice-example"));
    let audio = synth.synthesize(&text, &VoiceParams::default(), EnginePref::Windows, None)?;
    std::fs::write("speak.wav", &audio.bytes)?;
    println!("speak.wav: {} bytes (cached={})", audio.bytes.len(), audio.cached);
    Ok(())
}
