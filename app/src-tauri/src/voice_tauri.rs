//! Cola Tauri da voz: sink dos eventos `nyx://speech`/`nyx://speech-end`, a fala dos
//! toasts/chat/abertura, o "Mudo" da bandeja e os commands de voz.

use crate::model::{Settings, SpeechEvent, SpeechKind, VoiceStatus};
use crate::toast::Toast;
use crate::voice::{self, Voice};
use crate::AppState;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};

pub const EVT_SPEECH: &str = "nyx://speech";
pub const EVT_SPEECH_END: &str = "nyx://speech-end";

type CmdResult<T> = Result<T, String>;

pub struct TauriSpeechSink {
    pub app: AppHandle,
}

impl voice::SpeechSink for TauriSpeechSink {
    fn speech(&self, ev: &SpeechEvent) {
        let _ = self.app.emit(EVT_SPEECH, ev);
    }
    fn speech_end(&self, id: &str, interrupted: bool) {
        let _ = self.app.emit(EVT_SPEECH_END, serde_json::json!({ "id": id, "interrupted": interrupted }));
    }
}

/// Liga a voz real: `Synth` do nyx-voice com cache em `data_dir/tts-cache` e saída rodio.
pub fn spawn(app: &AppHandle, data_dir: &std::path::Path) -> anyhow::Result<Voice> {
    let cache = data_dir.join("tts-cache");
    let _ = std::fs::create_dir_all(&cache);
    Voice::spawn(voice::Deps {
        backend: Arc::new(nyx_voice::Synth::new(&cache)),
        sink: Arc::new(TauriSpeechSink { app: app.clone() }),
        out: Box::new(|| Box::new(voice::RodioOut::default()) as Box<dyn voice::AudioOut>),
        creds: nyx_voice::azure_creds_from_env,
        idle_release: voice::IDLE_RELEASE,
    })
}

fn dnd_now(s: &Settings) -> bool {
    crate::toast::dnd_active(s, chrono::Local::now().time())
}

/// Pede uma fala respeitando voz ligada, mudo, não perturbe e `voiceSpeakOn`.
pub fn say(app: &AppHandle, kind: SpeechKind, text: &str) -> Option<String> {
    let state = app.state::<AppState>();
    let s = state.settings();
    state.voice.say(&s, kind, text, dnd_now(&s))
}

/// A frase curta de um toast, se ele tiver uma.
pub fn say_toast(app: &AppHandle, t: &Toast) {
    if let Some(sp) = &t.speech {
        say(app, sp.kind, &sp.text);
    }
}

/// "Mudo" do menu da bandeja.
pub fn toggle_mute(app: &AppHandle) {
    let state = app.state::<AppState>();
    let mut s = state.settings();
    s.voice_muted = !s.voice_muted;
    if let Err(e) = crate::apply_settings(app, &state, s) {
        log::warn!("mudo: {e:#}");
    }
}

#[tauri::command]
pub async fn get_voice_status(app: AppHandle) -> CmdResult<VoiceStatus> {
    // a consulta da voz do Windows (WinRT) e o contador em disco ficam fora da thread principal
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        state.voice.status(&state.settings())
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn test_voice(app: AppHandle, text: Option<String>, voice_name: Option<String>) -> CmdResult<SpeechEvent> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        state.voice.test(&state.settings(), text.as_deref(), voice_name.as_deref())
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn set_voice_muted(app: AppHandle, muted: bool) -> CmdResult<Settings> {
    let state = app.state::<AppState>();
    let mut s = state.settings();
    s.voice_muted = muted;
    crate::apply_settings(&app, &state, s).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn stop_speech(app: AppHandle) -> CmdResult<()> {
    app.state::<AppState>().voice.stop();
    Ok(())
}

/// A UI chama ao abrir o painel com a mesma frase de abertura que mostra.
/// Devolve o id da fala pedida, ou `null` se as settings a calaram (mudo, não perturbe,
/// voz desligada, `voiceSpeakOn.opening` desligado). Repetida em < 20 s é descartada na fila.
#[tauri::command]
pub fn speak_opening(app: AppHandle, text: String) -> CmdResult<Option<String>> {
    Ok(say(&app, SpeechKind::Opening, &text))
}
