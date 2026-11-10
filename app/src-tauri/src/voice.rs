//! Voz da Nyx (TTS) no núcleo: fila numa thread própria, síntese pelo `nyx-voice`,
//! decodificação e playback com `rodio`, envelope da boca e eventos para a UI.
//! O áudio é do núcleo: toca com o painel fechado. Sem Tauri aqui (a cola fica em
//! `voice_tauri.rs`), para testar a fila com síntese, saída e sink falsos.
//!
//! Regras: `alert` interrompe `chat`/`opening`/`longTask`; mesma prioridade entra na
//! fila (máx. 3, descarta as mais velhas); a mesma frase em menos de 20 s não repete;
//! `test` fala na hora (ação explícita do usuário).

use crate::model::{Settings, SpeechEvent, SpeechKind, VoiceStatus};
use anyhow::{Context, Result};
use nyx_voice::{Audio, AzureCreds, Engine, EnginePref, VoiceParams};
use std::collections::VecDeque;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const MAX_PENDING: usize = 3;
pub const REPEAT_WINDOW: Duration = Duration::from_secs(20);
/// Fecha o dispositivo de áudio depois de tanto tempo sem falar (nada residente entre falas).
pub const IDLE_RELEASE: Duration = Duration::from_secs(5);
const POLL_WHILE_PLAYING: Duration = Duration::from_millis(50);
/// Teto do texto falado (economiza caracteres da Azure; a fala é sempre curta).
pub const MAX_SPOKEN_CHARS: usize = 300;
const TEST_TEXT: &str = "Teste de voz. É assim que eu soo.";
const TEST_TIMEOUT: Duration = Duration::from_secs(30);

// ---------- dependências (trocadas nos testes) ----------

/// O que o núcleo usa do `nyx-voice`.
pub trait TtsBackend: Send + Sync {
    fn synthesize(&self, text: &str, p: &VoiceParams, pref: EnginePref, creds: Option<&AzureCreds>) -> Result<Audio>;
    fn month_chars(&self) -> u64;
    fn windows_voice(&self) -> Option<String>;
    fn envelope(&self, mono: &[f32], sample_rate: u32) -> Vec<f32>;
}

impl TtsBackend for nyx_voice::Synth {
    fn synthesize(&self, text: &str, p: &VoiceParams, pref: EnginePref, creds: Option<&AzureCreds>) -> Result<Audio> {
        nyx_voice::Synth::synthesize(self, text, p, pref, creds)
    }
    fn month_chars(&self) -> u64 {
        nyx_voice::Synth::month_chars(self)
    }
    fn windows_voice(&self) -> Option<String> {
        nyx_voice::windows_voice_name()
    }
    fn envelope(&self, mono: &[f32], sample_rate: u32) -> Vec<f32> {
        nyx_voice::envelope_from_pcm(mono, sample_rate)
    }
}

/// Saída de áudio. Criada dentro da thread da voz (o `OutputStream` do rodio não é `Send`).
pub trait AudioOut {
    /// Começa a tocar agora (abre o dispositivo se preciso).
    fn play(&mut self, pcm: Pcm, volume: f32) -> Result<()>;
    fn is_playing(&self) -> bool;
    fn stop(&mut self);
    fn is_open(&self) -> bool;
    /// Fecha o dispositivo (libera o stream e os buffers).
    fn release(&mut self);
}

/// Recebe os eventos da fala (a cola do Tauri emite `nyx://speech` / `nyx://speech-end`).
pub trait SpeechSink: Send + Sync {
    fn speech(&self, ev: &SpeechEvent);
    fn speech_end(&self, id: &str, interrupted: bool);
}

pub type OutFactory = Box<dyn FnOnce() -> Box<dyn AudioOut> + Send>;

pub struct Deps {
    pub backend: Arc<dyn TtsBackend>,
    pub sink: Arc<dyn SpeechSink>,
    pub out: OutFactory,
    /// Lidas a cada fala (pega a chave nova depois de reiniciar).
    pub creds: fn() -> Option<AzureCreds>,
    pub idle_release: Duration,
}

// ---------- PCM ----------

/// Áudio decodificado: amostras intercaladas para tocar e o mono para o envelope.
#[derive(Debug, Clone)]
pub struct Pcm {
    pub channels: u16,
    pub sample_rate: u32,
    pub samples: Vec<i16>,
}

impl Pcm {
    pub fn duration_ms(&self) -> u64 {
        let frames = self.samples.len() as u64 / self.channels.max(1) as u64;
        frames * 1000 / self.sample_rate.max(1) as u64
    }

    pub fn mono(&self) -> Vec<f32> {
        let ch = self.channels.max(1) as usize;
        self.samples
            .chunks(ch)
            .map(|f| f.iter().map(|&s| s as f32 / 32768.0).sum::<f32>() / f.len() as f32)
            .collect()
    }
}

/// MP3 (Azure) ou WAV (Windows) -> PCM, pelo decodificador do rodio (symphonia).
pub fn decode(bytes: Vec<u8>) -> Result<Pcm> {
    use rodio::Source;
    let dec = rodio::Decoder::new(std::io::Cursor::new(bytes)).context("áudio da síntese ilegível")?;
    let channels = dec.channels();
    let sample_rate = dec.sample_rate();
    let samples: Vec<i16> = dec.collect();
    anyhow::ensure!(!samples.is_empty() && channels > 0 && sample_rate > 0, "a síntese devolveu áudio vazio");
    Ok(Pcm { channels, sample_rate, samples })
}

/// Saída real com rodio. Abre o stream na primeira fala e o fecha via `release`.
#[derive(Default)]
pub struct RodioOut {
    stream: Option<(rodio::OutputStream, rodio::OutputStreamHandle)>,
    sink: Option<rodio::Sink>,
}

impl AudioOut for RodioOut {
    fn play(&mut self, pcm: Pcm, volume: f32) -> Result<()> {
        self.stop();
        if self.stream.is_none() {
            self.stream = Some(rodio::OutputStream::try_default().context("sem dispositivo de áudio")?);
        }
        let handle = &self.stream.as_ref().unwrap().1;
        let sink = rodio::Sink::try_new(handle).context("não abriu a saída de áudio")?;
        sink.set_volume(volume.clamp(0.0, 1.0));
        sink.append(rodio::buffer::SamplesBuffer::new(pcm.channels, pcm.sample_rate, pcm.samples));
        self.sink = Some(sink);
        Ok(())
    }
    fn is_playing(&self) -> bool {
        self.sink.as_ref().is_some_and(|s| !s.empty())
    }
    fn stop(&mut self) {
        if let Some(s) = self.sink.take() {
            s.stop();
        }
    }
    fn is_open(&self) -> bool {
        self.stream.is_some()
    }
    fn release(&mut self) {
        self.stop();
        self.stream = None;
    }
}

// ---------- quando fala ----------

/// A Nyx pode falar este tipo agora? `test` ignora mudo e não perturbe, mas nunca
/// fala com a voz desligada.
pub fn allowed(s: &Settings, kind: SpeechKind, dnd: bool) -> bool {
    if !s.voice_enabled {
        return false;
    }
    if kind == SpeechKind::Test {
        return true;
    }
    if s.voice_muted || dnd {
        return false;
    }
    let on = &s.voice_speak_on;
    match kind {
        SpeechKind::Alert => on.alerts,
        SpeechKind::Opening => on.opening,
        SpeechKind::Chat => on.chat,
        SpeechKind::LongTask => on.long_task,
        SpeechKind::Test => true,
    }
}

/// Texto falado: sem blocos de código, espaços colapsados e cortado em frase inteira
/// até `MAX_SPOKEN_CHARS`. `None` se não sobrar nada.
pub fn spoken_text(text: &str) -> Option<String> {
    let mut kept = String::new();
    let mut in_code = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if !in_code {
            kept.push_str(line);
            kept.push(' ');
        }
    }
    let flat = kept.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.is_empty() {
        return None;
    }
    if flat.chars().count() <= MAX_SPOKEN_CHARS {
        return Some(flat);
    }
    let head: String = flat.chars().take(MAX_SPOKEN_CHARS).collect();
    // último fim de frase dentro do limite; senão, última palavra inteira
    let cut = head
        .char_indices()
        .filter(|&(i, c)| matches!(c, '.' | '?' | '!') && head[i + c.len_utf8()..].starts_with(' '))
        .map(|(i, c)| i + c.len_utf8())
        .next_back()
        .filter(|&i| i >= 40);
    Some(match cut {
        Some(i) => head[..i].to_string(),
        None => head.rsplit_once(' ').map(|(a, _)| a.to_string()).unwrap_or(head),
    })
}

fn dedup_key(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// Config da fala tirada das settings no momento do pedido.
#[derive(Debug, Clone)]
pub struct VoiceConfig {
    pub params: VoiceParams,
    pub pref: EnginePref,
    pub volume: f32,
}

impl VoiceConfig {
    pub fn from_settings(s: &Settings) -> Self {
        Self {
            params: VoiceParams { voice_name: s.voice_name.clone(), rate_pct: s.voice_rate, pitch_pct: s.voice_pitch },
            pref: s.voice_engine,
            volume: s.voice_volume as f32,
        }
    }
}

// ---------- fila (lógica pura) ----------

pub struct Request {
    pub id: String,
    pub kind: SpeechKind,
    pub text: String,
    pub cfg: VoiceConfig,
    /// Só o `test_voice`: recebe o evento quando o áudio começa (ou o erro).
    pub reply: Option<mpsc::Sender<std::result::Result<SpeechEvent, String>>>,
}

impl Request {
    fn fail(self, msg: &str) {
        if let Some(r) = self.reply {
            let _ = r.send(Err(msg.to_string()));
        }
    }
}

fn rank(k: SpeechKind) -> u8 {
    match k {
        SpeechKind::Test => 2,
        SpeechKind::Alert => 1,
        SpeechKind::Opening | SpeechKind::Chat | SpeechKind::LongTask => 0,
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Admit {
    /// Repetida em menos de 20 s.
    Repeated,
    Queued,
    /// Entrou na frente e a fala atual deve ser interrompida.
    Interrupt,
}

#[derive(Default)]
pub struct Queue {
    pending: VecDeque<Request>,
    recent: VecDeque<(String, Instant)>,
}

impl Queue {
    /// `current` = tipo da fala tocando agora, se houver.
    pub fn admit(&mut self, req: Request, current: Option<SpeechKind>, now: Instant) -> Admit {
        if req.kind != SpeechKind::Test {
            while self.recent.front().is_some_and(|(_, t)| now.duration_since(*t) >= REPEAT_WINDOW) {
                self.recent.pop_front();
            }
            let key = dedup_key(&req.text);
            if self.recent.iter().any(|(k, _)| *k == key) {
                req.fail("a mesma frase acabou de ser falada");
                return Admit::Repeated;
            }
            self.recent.push_back((key, now));
        }
        let r = rank(req.kind);
        if current.is_some_and(|c| r > rank(c)) {
            self.pending.push_front(req);
            self.trim();
            return Admit::Interrupt;
        }
        // depois da última de prioridade igual ou maior (FIFO dentro da mesma prioridade)
        let pos = self.pending.iter().rposition(|q| rank(q.kind) >= r).map_or(0, |i| i + 1);
        self.pending.insert(pos, req);
        self.trim();
        Admit::Queued
    }

    /// Máximo `MAX_PENDING`: sai a mais velha da prioridade mais baixa.
    fn trim(&mut self) {
        while self.pending.len() > MAX_PENDING {
            let low = self.pending.iter().map(|q| rank(q.kind)).min().unwrap_or(0);
            let i = self.pending.iter().position(|q| rank(q.kind) == low).unwrap_or(0);
            if let Some(old) = self.pending.remove(i) {
                log::debug!("voz: fila cheia, descartada: {}", old.text);
                old.fail("descartada: fila de fala cheia");
            }
        }
    }

    pub fn pop(&mut self) -> Option<Request> {
        self.pending.pop_front()
    }

    pub fn clear(&mut self) {
        for r in self.pending.drain(..) {
            r.fail("fala interrompida");
        }
    }

    pub fn len(&self) -> usize {
        self.pending.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    pub fn texts(&self) -> Vec<String> {
        self.pending.iter().map(|q| q.text.clone()).collect()
    }
}

// ---------- worker ----------

enum Cmd {
    Speak(Request),
    Stop,
}

#[derive(Default)]
struct Shared {
    speaking: bool,
    last_engine: Option<Engine>,
    last_error: Option<String>,
}

/// Handle da voz no `AppState`.
pub struct Voice {
    tx: mpsc::Sender<Cmd>,
    shared: Arc<Mutex<Shared>>,
    backend: Arc<dyn TtsBackend>,
    creds: fn() -> Option<AzureCreds>,
}

impl Voice {
    pub fn spawn(deps: Deps) -> Result<Voice> {
        let (tx, rx) = mpsc::channel();
        let shared = Arc::new(Mutex::new(Shared::default()));
        let voice = Voice { tx, shared: shared.clone(), backend: deps.backend.clone(), creds: deps.creds };
        std::thread::Builder::new().name("nyx-voice".into()).spawn(move || {
            let out = (deps.out)();
            Worker {
                rx,
                backend: deps.backend,
                sink: deps.sink,
                out,
                creds: deps.creds,
                idle_release: deps.idle_release,
                shared,
                queue: Queue::default(),
                current: None,
                idle_since: Instant::now(),
            }
            .run();
        })?;
        Ok(voice)
    }

    /// Pede uma fala se as settings deixam (`dnd` = não perturbe ativo agora).
    /// Devolve o id da fala aceita na fila.
    pub fn say(&self, s: &Settings, kind: SpeechKind, text: &str, dnd: bool) -> Option<String> {
        if kind == SpeechKind::Test || !allowed(s, kind, dnd) {
            return None;
        }
        let text = spoken_text(text)?;
        let id = crate::core::new_id();
        let req = Request { id: id.clone(), kind, text, cfg: VoiceConfig::from_settings(s), reply: None };
        self.tx.send(Cmd::Speak(req)).ok()?;
        Some(id)
    }

    /// "Testar voz": fala na hora (mesmo no mudo/não perturbe) e devolve o evento
    /// quando o áudio começa. Bloqueia: chame fora da thread principal.
    pub fn test(&self, s: &Settings, text: Option<&str>, voice_name: Option<&str>) -> Result<SpeechEvent> {
        anyhow::ensure!(
            s.voice_enabled,
            "A voz está desligada. Ligue a voz nas configurações para testar."
        );
        let text = text.and_then(spoken_text).unwrap_or_else(|| TEST_TEXT.to_string());
        let mut cfg = VoiceConfig::from_settings(s);
        if let Some(v) = voice_name.filter(|v| !v.trim().is_empty()) {
            cfg.params.voice_name =
                crate::core::valid_voice_name(v).ok_or_else(|| anyhow::anyhow!("Nome de voz inválido: {v}"))?;
        }
        let (rtx, rrx) = mpsc::channel();
        let req = Request { id: crate::core::new_id(), kind: SpeechKind::Test, text, cfg, reply: Some(rtx) };
        self.tx.send(Cmd::Speak(req)).map_err(|_| anyhow::anyhow!("a thread da voz parou"))?;
        match rrx.recv_timeout(TEST_TIMEOUT) {
            Ok(r) => r.map_err(|e| anyhow::anyhow!(e)),
            Err(_) => anyhow::bail!("a síntese demorou demais"),
        }
    }

    /// Para a fala atual e esvazia a fila.
    pub fn stop(&self) {
        let _ = self.tx.send(Cmd::Stop);
    }

    pub fn status(&self, s: &Settings) -> VoiceStatus {
        let creds = (self.creds)();
        let windows_voice = self.backend.windows_voice();
        let sh = self.shared.lock().unwrap();
        let predicted = match s.voice_engine {
            EnginePref::Azure => Some(Engine::Azure),
            EnginePref::Windows => windows_voice.as_ref().map(|_| Engine::Windows),
            EnginePref::Auto if creds.is_some() => Some(Engine::Azure),
            EnginePref::Auto => windows_voice.as_ref().map(|_| Engine::Windows),
        };
        // a última fala real vale mais que a previsão (o auto pode ter caído na Windows)
        let engine = if s.voice_enabled { sh.last_engine.or(predicted) } else { None };
        VoiceStatus {
            engine_in_use: match engine {
                Some(Engine::Azure) => "azure",
                Some(Engine::Windows) => "windows",
                None => "none",
            }
            .to_string(),
            azure_key_present: creds.is_some(),
            azure_region: creds.map(|c| c.region),
            windows_voice,
            month_chars: self.backend.month_chars(),
            free_tier_chars: nyx_voice::FREE_TIER_CHARS,
            speaking: sh.speaking,
            last_error: sh.last_error.clone(),
        }
    }
}

struct Worker {
    rx: mpsc::Receiver<Cmd>,
    backend: Arc<dyn TtsBackend>,
    sink: Arc<dyn SpeechSink>,
    out: Box<dyn AudioOut>,
    creds: fn() -> Option<AzureCreds>,
    idle_release: Duration,
    shared: Arc<Mutex<Shared>>,
    queue: Queue,
    current: Option<(String, SpeechKind)>,
    idle_since: Instant,
}

impl Worker {
    fn run(mut self) {
        loop {
            let wait = if self.current.is_some() {
                Some(POLL_WHILE_PLAYING)
            } else if !self.queue.is_empty() {
                Some(Duration::ZERO)
            } else if self.out.is_open() {
                Some(self.idle_release.saturating_sub(self.idle_since.elapsed()))
            } else {
                None // dorme até o próximo pedido: 0% de CPU em repouso
            };
            let cmd = match wait {
                None => match self.rx.recv() {
                    Ok(c) => Some(c),
                    Err(_) => break,
                },
                Some(t) => match self.rx.recv_timeout(t) {
                    Ok(c) => Some(c),
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(RecvTimeoutError::Disconnected) => break,
                },
            };
            match cmd {
                Some(Cmd::Speak(req)) => {
                    let cur = self.current.as_ref().map(|c| c.1);
                    if self.queue.admit(req, cur, Instant::now()) == Admit::Interrupt {
                        self.end_current(true);
                    }
                }
                Some(Cmd::Stop) => {
                    self.queue.clear();
                    self.end_current(true);
                }
                None => {}
            }
            if self.current.is_some() && !self.out.is_playing() {
                self.end_current(false);
            }
            while self.current.is_none() {
                let Some(req) = self.queue.pop() else { break };
                self.start(req);
            }
            if self.current.is_none() && self.out.is_open() && self.idle_since.elapsed() >= self.idle_release {
                log::debug!("voz: ociosa, fechando o dispositivo de áudio");
                self.out.release();
            }
        }
        self.out.release();
    }

    fn end_current(&mut self, interrupted: bool) {
        let Some((id, _)) = self.current.take() else { return };
        if interrupted {
            self.out.stop();
        }
        self.shared.lock().unwrap().speaking = false;
        self.idle_since = Instant::now();
        self.sink.speech_end(&id, interrupted);
    }

    fn start(&mut self, req: Request) {
        let creds = (self.creds)();
        let backend = self.backend.clone();
        let (text, cfg) = (req.text.clone(), req.cfg.clone());
        // um pânico na síntese não pode matar a thread da voz
        let synth = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            backend.synthesize(&text, &cfg.params, cfg.pref, creds.as_ref())
        }))
        .unwrap_or_else(|_| Err(anyhow::anyhow!("falha interna na síntese")));
        let prepared = synth.and_then(|audio| {
            let engine = audio.engine;
            decode(audio.bytes).map(|pcm| (engine, pcm))
        });
        let (engine, pcm) = match prepared {
            Ok(p) => p,
            Err(e) => return self.fail(req, format!("{e:#}")),
        };
        let envelope = backend.envelope(&pcm.mono(), pcm.sample_rate);
        let ev = SpeechEvent {
            id: req.id.clone(),
            kind: req.kind,
            text: req.text.clone(),
            engine,
            duration_ms: pcm.duration_ms(),
            envelope,
        };
        if let Err(e) = self.out.play(pcm, cfg.volume) {
            return self.fail(req, format!("{e:#}"));
        }
        {
            let mut sh = self.shared.lock().unwrap();
            sh.speaking = true;
            sh.last_engine = Some(engine);
            sh.last_error = None;
        }
        self.current = Some((req.id.clone(), req.kind));
        self.sink.speech(&ev);
        if let Some(r) = req.reply {
            let _ = r.send(Ok(ev));
        }
    }

    fn fail(&mut self, req: Request, msg: String) {
        log::warn!("voz: {msg}");
        self.shared.lock().unwrap().last_error = Some(msg.clone());
        self.idle_since = Instant::now();
        req.fail(&msg);
    }
}

// ---------- testes ----------

#[cfg(test)]
pub mod testing {
    //! Síntese, saída e sink falsos (WAV de seno gerado em memória).
    use super::*;

    pub fn sine_wav(ms: u32, rate: u32) -> Vec<u8> {
        let n = (rate as u64 * ms as u64 / 1000) as u32;
        let data_len = n * 2;
        let mut b = Vec::with_capacity(44 + data_len as usize);
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + data_len).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes()); // PCM
        b.extend_from_slice(&1u16.to_le_bytes()); // mono
        b.extend_from_slice(&rate.to_le_bytes());
        b.extend_from_slice(&(rate * 2).to_le_bytes());
        b.extend_from_slice(&2u16.to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&data_len.to_le_bytes());
        for i in 0..n {
            let v = (i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin() * 12000.0;
            b.extend_from_slice(&(v as i16).to_le_bytes());
        }
        b
    }

    /// Duração do áudio falso: "longa" no texto = 1500 ms, senão 150 ms. "falha" = erro.
    pub struct FakeTts {
        pub calls: Mutex<Vec<(String, String)>>,
    }

    impl FakeTts {
        pub fn new() -> Arc<Self> {
            Arc::new(Self { calls: Mutex::new(vec![]) })
        }
    }

    impl TtsBackend for FakeTts {
        fn synthesize(&self, text: &str, p: &VoiceParams, _: EnginePref, _: Option<&AzureCreds>) -> Result<Audio> {
            self.calls.lock().unwrap().push((text.to_string(), p.voice_name.clone()));
            anyhow::ensure!(!text.contains("falha"), "síntese falsa falhou");
            let ms = if text.contains("longa") { 1500 } else { 150 };
            Ok(Audio { engine: Engine::Windows, format: nyx_voice::AudioFormat::Wav, bytes: sine_wav(ms, 16000), cached: false })
        }
        fn month_chars(&self) -> u64 {
            42
        }
        fn windows_voice(&self) -> Option<String> {
            Some("Microsoft Maria".into())
        }
        fn envelope(&self, mono: &[f32], rate: u32) -> Vec<f32> {
            let step = (rate as usize * nyx_voice::ENVELOPE_STEP_MS as usize / 1000).max(1);
            mono.chunks(step).map(|c| c.iter().fold(0f32, |m, s| m.max(s.abs()))).collect()
        }
    }

    #[derive(Default)]
    pub struct OutLog {
        pub events: Vec<String>,
    }

    pub struct FakeOut {
        pub log: Arc<Mutex<OutLog>>,
        until: Option<Instant>,
        open: bool,
    }

    impl FakeOut {
        pub fn new(log: Arc<Mutex<OutLog>>) -> Self {
            Self { log, until: None, open: false }
        }
    }

    impl AudioOut for FakeOut {
        fn play(&mut self, pcm: Pcm, volume: f32) -> Result<()> {
            self.open = true;
            self.until = Some(Instant::now() + Duration::from_millis(pcm.duration_ms()));
            self.log.lock().unwrap().events.push(format!("play {}ms vol {volume:.1}", pcm.duration_ms()));
            Ok(())
        }
        fn is_playing(&self) -> bool {
            self.until.is_some_and(|u| Instant::now() < u)
        }
        fn stop(&mut self) {
            if self.until.take().is_some() {
                self.log.lock().unwrap().events.push("stop".into());
            }
        }
        fn is_open(&self) -> bool {
            self.open
        }
        fn release(&mut self) {
            if self.open {
                self.open = false;
                self.log.lock().unwrap().events.push("release".into());
            }
        }
    }

    #[derive(Default)]
    pub struct FakeSink {
        pub events: Mutex<Vec<String>>,
    }

    impl SpeechSink for FakeSink {
        fn speech(&self, ev: &SpeechEvent) {
            self.events.lock().unwrap().push(format!("speech {:?} {}", ev.kind, ev.text));
        }
        fn speech_end(&self, _id: &str, interrupted: bool) {
            self.events.lock().unwrap().push(format!("end {interrupted}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::*;
    use super::*;
    use crate::model::Settings;

    fn req(kind: SpeechKind, text: &str) -> Request {
        Request {
            id: text.into(),
            kind,
            text: text.into(),
            cfg: VoiceConfig::from_settings(&Settings::defaults(1.0)),
            reply: None,
        }
    }

    #[test]
    fn queue_priority_limit_and_repeat() {
        let mut q = Queue::default();
        let t0 = Instant::now();
        assert_eq!(q.admit(req(SpeechKind::Chat, "c1"), None, t0), Admit::Queued);
        assert_eq!(q.admit(req(SpeechKind::Opening, "o1"), None, t0), Admit::Queued);
        assert_eq!(q.admit(req(SpeechKind::Alert, "a1"), None, t0), Admit::Queued);
        assert_eq!(q.texts(), vec!["a1", "c1", "o1"]); // alerta passa na frente
        // cheia: sai a mais velha da prioridade mais baixa
        assert_eq!(q.admit(req(SpeechKind::LongTask, "l1"), None, t0), Admit::Queued);
        assert_eq!(q.texts(), vec!["a1", "o1", "l1"]);
        assert_eq!(q.len(), MAX_PENDING);
        // a mesma frase em < 20 s não repete (nem com outra caixa/espaços)
        assert_eq!(q.admit(req(SpeechKind::Chat, "  C1 "), None, t0 + Duration::from_secs(19)), Admit::Repeated);
        // alerta interrompe chat/abertura/tarefa longa em andamento
        assert_eq!(q.admit(req(SpeechKind::Alert, "a2"), Some(SpeechKind::Chat), t0), Admit::Interrupt);
        assert_eq!(q.pop().unwrap().text, "a2");
        // alerta não interrompe outro alerta; entra na fila depois dos alertas
        assert_eq!(q.admit(req(SpeechKind::Alert, "a3"), Some(SpeechKind::Alert), t0), Admit::Queued);
        assert_eq!(q.texts(), vec!["a1", "a3", "l1"]); // cheia: saiu o1, a mais velha das normais
        // chat não interrompe nada
        assert_eq!(q.admit(req(SpeechKind::Chat, "c9"), Some(SpeechKind::LongTask), t0), Admit::Queued);
        // depois de 20 s pode repetir
        let mut q2 = Queue::default();
        q2.admit(req(SpeechKind::Chat, "x"), None, t0);
        q2.pop();
        assert_eq!(q2.admit(req(SpeechKind::Chat, "x"), None, t0 + REPEAT_WINDOW), Admit::Queued);
        // teste fala sempre e passa na frente de tudo
        assert_eq!(q2.admit(req(SpeechKind::Test, "x"), Some(SpeechKind::Alert), t0), Admit::Interrupt);
        assert_eq!(q2.admit(req(SpeechKind::Test, "x"), None, t0), Admit::Queued);
    }

    #[test]
    fn when_she_speaks() {
        let mut s = Settings::defaults(1.0);
        for k in [SpeechKind::Alert, SpeechKind::Opening, SpeechKind::Chat, SpeechKind::LongTask, SpeechKind::Test] {
            assert!(allowed(&s, k, false), "{k:?}");
            // não perturbe cala tudo, menos o teste
            assert_eq!(allowed(&s, k, true), k == SpeechKind::Test, "{k:?}");
        }
        s.voice_speak_on.chat = false;
        assert!(!allowed(&s, SpeechKind::Chat, false));
        assert!(allowed(&s, SpeechKind::Alert, false));
        s.voice_speak_on.alerts = false;
        s.voice_speak_on.opening = false;
        s.voice_speak_on.long_task = false;
        assert!(!allowed(&s, SpeechKind::Alert, false));
        assert!(!allowed(&s, SpeechKind::Opening, false));
        assert!(!allowed(&s, SpeechKind::LongTask, false));
        let mut s = Settings::defaults(1.0);
        s.voice_muted = true;
        assert!(!allowed(&s, SpeechKind::Alert, false));
        assert!(allowed(&s, SpeechKind::Test, false)); // ação explícita
        s.voice_enabled = false;
        assert!(!allowed(&s, SpeechKind::Test, false));
        // não perturbe no horário
        let mut s = Settings::defaults(1.0);
        s.dnd_schedule = Some(crate::model::DndSchedule { start: "22:00".into(), end: "08:00".into() });
        let dnd = crate::toast::dnd_active(&s, chrono::NaiveTime::from_hms_opt(23, 0, 0).unwrap());
        assert!(!allowed(&s, SpeechKind::Alert, dnd));
    }

    #[test]
    fn spoken_text_is_short_and_clean() {
        assert_eq!(spoken_text("  Etapa 1 de 3 concluída.\n Aprova? ").unwrap(), "Etapa 1 de 3 concluída. Aprova?");
        assert_eq!(spoken_text("Veja:\n```rust\nfn main() {}\n```\nPronto.").unwrap(), "Veja: Pronto.");
        assert!(spoken_text(" \n ").is_none());
        let long = "Frase de abertura bem comprida aqui. ".repeat(20);
        let s = spoken_text(&long).unwrap();
        assert!(s.chars().count() <= MAX_SPOKEN_CHARS && s.ends_with('.'), "{s}");
        let words = "palavra ".repeat(80);
        let s = spoken_text(&words).unwrap();
        assert!(s.chars().count() <= MAX_SPOKEN_CHARS && s.ends_with("palavra"));
    }

    #[test]
    fn decode_sine_wav_and_envelope() {
        let pcm = decode(sine_wav(500, 16000)).unwrap();
        assert_eq!((pcm.channels, pcm.sample_rate), (1, 16000));
        assert_eq!(pcm.duration_ms(), 500);
        let env = FakeTts::new().envelope(&pcm.mono(), pcm.sample_rate);
        assert_eq!(env.len(), 10);
        assert!(decode(b"not audio".to_vec()).is_err());
    }

    struct Rig {
        voice: Voice,
        sink: Arc<FakeSink>,
        out: Arc<Mutex<OutLog>>,
        tts: Arc<FakeTts>,
    }

    fn rig(idle: Duration) -> Rig {
        let sink = Arc::new(FakeSink::default());
        let out = Arc::new(Mutex::new(OutLog::default()));
        let tts = FakeTts::new();
        let o = out.clone();
        let voice = Voice::spawn(Deps {
            backend: tts.clone(),
            sink: sink.clone(),
            out: Box::new(move || Box::new(FakeOut::new(o)) as Box<dyn AudioOut>),
            creds: || None,
            idle_release: idle,
        })
        .unwrap();
        Rig { voice, sink, out, tts }
    }

    fn wait_for(f: impl Fn() -> bool) {
        let t = Instant::now();
        while !f() {
            assert!(t.elapsed() < Duration::from_secs(5), "timeout");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn worker_alert_interrupts_chat_and_releases_device() {
        let r = rig(Duration::from_millis(300));
        let s = Settings::defaults(1.0);
        assert!(r.voice.say(&s, SpeechKind::Chat, "uma resposta longa do chat", false).is_some());
        wait_for(|| r.sink.events.lock().unwrap().len() == 1);
        assert!(r.voice.status(&s).speaking);
        assert!(r.voice.say(&s, SpeechKind::Alert, "Aguardando você", false).is_some());
        wait_for(|| r.sink.events.lock().unwrap().len() == 4);
        assert_eq!(
            *r.sink.events.lock().unwrap(),
            vec!["speech Chat uma resposta longa do chat", "end true", "speech Alert Aguardando você", "end false"]
        );
        // repetida em < 20 s: não fala
        assert!(r.voice.say(&s, SpeechKind::Alert, "Aguardando você", false).is_some());
        // ociosa: fecha o dispositivo
        wait_for(|| r.out.lock().unwrap().events.last().map(String::as_str) == Some("release"));
        assert_eq!(r.sink.events.lock().unwrap().len(), 4);
        assert_eq!(r.out.lock().unwrap().events[0], "play 1500ms vol 0.8");
        let st = r.voice.status(&s);
        assert!(!st.speaking);
        assert_eq!(st.engine_in_use, "windows");
        assert_eq!((st.month_chars, st.free_tier_chars), (42, 500_000));
        assert!(!st.azure_key_present && st.azure_region.is_none());
    }

    #[test]
    fn worker_silence_rules_and_test_voice() {
        let r = rig(Duration::from_millis(100));
        let mut s = Settings::defaults(1.0);
        s.voice_muted = true;
        assert!(r.voice.say(&s, SpeechKind::Alert, "calada", false).is_none());
        s.voice_muted = false;
        assert!(r.voice.say(&s, SpeechKind::Alert, "calada", true).is_none()); // não perturbe
        // teste fala mesmo no mudo, com a voz pedida, e devolve o evento
        s.voice_muted = true;
        let ev = r.voice.test(&s, None, Some("pt-BR-ThalitaNeural")).unwrap();
        assert_eq!(ev.kind, SpeechKind::Test);
        assert_eq!(ev.text, TEST_TEXT);
        assert_eq!(ev.duration_ms, 150);
        assert_eq!(ev.envelope.len(), 3);
        assert_eq!(r.tts.calls.lock().unwrap()[0].1, "pt-BR-ThalitaNeural");
        assert!(r.voice.test(&s, Some("x"), Some("bad name<")).is_err());
        let err = r.voice.test(&s, Some("falha"), None).unwrap_err().to_string();
        assert!(err.contains("falhou"), "{err}");
        assert!(r.voice.status(&s).last_error.is_some());
        s.voice_enabled = false;
        assert!(r.voice.test(&s, None, None).unwrap_err().to_string().contains("desligada"));
        assert_eq!(r.voice.status(&s).engine_in_use, "none");
        // stop: interrompe e esvazia
        s.voice_enabled = true;
        s.voice_muted = false;
        r.voice.say(&s, SpeechKind::Chat, "fala longa um", false);
        r.voice.say(&s, SpeechKind::Chat, "fala longa dois", false);
        wait_for(|| r.sink.events.lock().unwrap().iter().any(|e| e.contains("longa um")));
        r.voice.stop();
        wait_for(|| r.sink.events.lock().unwrap().last().map(String::as_str) == Some("end true"));
        std::thread::sleep(Duration::from_millis(200));
        assert!(!r.sink.events.lock().unwrap().iter().any(|e| e.contains("longa dois")));
    }
    /// (working set, private bytes) em MB.
    fn working_set_mb() -> (f64, f64) {
        let out = std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", &format!("$p=Get-Process -Id {}; \"$($p.WorkingSet64) $($p.PrivateMemorySize64)\"", std::process::id())])
            .output()
            .unwrap();
        let txt = String::from_utf8_lossy(&out.stdout).to_string();
        let v: Vec<f64> = txt.split_whitespace().map(|x| x.parse::<f64>().unwrap_or(0.0) / 1048576.0).collect();
        (v.first().copied().unwrap_or(0.0), v.get(1).copied().unwrap_or(0.0))
    }

    /// Fala de verdade (Maria, sem chave) pela saída rodio e mede a RAM antes, durante e
    /// depois do fechamento do dispositivo. `cargo test -p nyx real_voice -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_voice_smoke_and_ram() {
        let dir = tempfile::tempdir().unwrap();
        let sink = Arc::new(FakeSink::default());
        let voice = Voice::spawn(Deps {
            backend: Arc::new(nyx_voice::Synth::new(dir.path())),
            sink: sink.clone(),
            out: Box::new(|| Box::new(RodioOut::default()) as Box<dyn AudioOut>),
            creds: || None,
            idle_release: IDLE_RELEASE,
        })
        .unwrap();
        let s = Settings::defaults(1.0);
        let mut rows = vec![("antes", working_set_mb())];
        for (i, text) in ["Etapa 2 de 3 concluída. Aprova?", "Aguardando você.", "Semana em 82% do teto."].iter().enumerate() {
            voice.say(&s, SpeechKind::Alert, text, false).unwrap();
            wait_for(|| sink.events.lock().unwrap().len() > i * 2);
            rows.push(("falando", working_set_mb()));
            let t = Instant::now();
            while sink.events.lock().unwrap().len() < (i + 1) * 2 {
                assert!(t.elapsed() < Duration::from_secs(20));
                std::thread::sleep(Duration::from_millis(50));
            }
            std::thread::sleep(IDLE_RELEASE + Duration::from_secs(2));
            rows.push(("ociosa 7 s", working_set_mb()));
        }
        println!("eventos {:?}", sink.events.lock().unwrap());
        println!("status {:?}", voice.status(&s));
        for (k, (ws, pb)) in rows {
            println!("RAM {k}: working set {ws:.1} MB, privada {pb:.1} MB");
        }
        assert_eq!(sink.events.lock().unwrap()[1], "end false");
    }
}
