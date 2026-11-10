//! Instalação dos hooks do Claude Code pelo painel: wrappers finos de `nyx_claude::hooks`.
//! O caminho do settings é sempre recebido por parâmetro (os testes usam temporários).

use nyx_claude::hooks::{self, HooksReport, HooksStatus};
use std::path::{Path, PathBuf};

/// Pasta dos sidecars em dev (`app/src-tauri/binaries`); não existe no app instalado.
fn dev_binaries_dir() -> Option<PathBuf> {
    cfg!(debug_assertions).then(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("binaries"))
}

/// `nyx-hook.exe` ao lado do `nyx.exe` (onde o instalador põe o sidecar). Em dev, se não
/// existir, usa `binaries/nyx-hook-<triple>.exe`. Sem nenhum dos dois, devolve o vizinho
/// (o status mostra `hookExeExists: false`).
pub fn resolve_hook_exe(sibling: PathBuf, dev_dir: Option<&Path>) -> PathBuf {
    if sibling.exists() {
        return sibling;
    }
    let dev = dev_dir.and_then(|d| std::fs::read_dir(d).ok()).and_then(|rd| {
        rd.flatten().map(|e| e.path()).find(|p| {
            let name = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
            name.starts_with("nyx-hook-") && (name.ends_with(".exe") || !cfg!(windows))
        })
    });
    dev.map(|p| hooks::clean_path(&p)).unwrap_or(sibling)
}

pub fn hook_exe() -> PathBuf {
    resolve_hook_exe(hooks::sibling_hook_exe(), dev_binaries_dir().as_deref())
}

pub fn status_at(settings: &Path, exe: &Path) -> anyhow::Result<HooksStatus> {
    hooks::status(settings, exe)
}

pub fn apply_at(settings: &Path, exe: &Path, install: bool, dry_run: bool) -> anyhow::Result<HooksReport> {
    let r = hooks::apply(settings, exe, install, dry_run)?;
    log::info!(
        "hooks {}{}: changed={} backup={:?}",
        if install { "install" } else { "uninstall" },
        if dry_run { " (dry-run)" } else { "" },
        r.changed,
        r.backup_path
    );
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_sibling_then_dev_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let sibling = dir.path().join("app").join("nyx-hook.exe");
        let bins = dir.path().join("binaries");
        std::fs::create_dir_all(&bins).unwrap();
        // nada existe: devolve o vizinho
        assert_eq!(resolve_hook_exe(sibling.clone(), Some(&bins)), sibling);
        std::fs::write(bins.join("nyx-hook-x86_64-pc-windows-msvc.exe"), b"").unwrap();
        assert!(resolve_hook_exe(sibling.clone(), Some(&bins))
            .ends_with("nyx-hook-x86_64-pc-windows-msvc.exe"));
        std::fs::create_dir_all(sibling.parent().unwrap()).unwrap();
        std::fs::write(&sibling, b"").unwrap();
        assert_eq!(resolve_hook_exe(sibling.clone(), Some(&bins)), sibling);
    }

    #[test]
    fn install_dry_run_then_real_on_temp_settings() {
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join("settings.json");
        // um hook de terceiro (ex.: Orca) que não pode ser tocado
        std::fs::write(
            &settings,
            r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"orca-hook stop"}]}]}}"#,
        )
        .unwrap();
        let exe = dir.path().join("nyx-hook.exe");
        std::fs::write(&exe, b"").unwrap();

        let st = status_at(&settings, &exe).unwrap();
        assert!(!st.installed);
        assert_eq!(st.other_hooks, 1);

        let before = std::fs::read(&settings).unwrap();
        let dry = apply_at(&settings, &exe, true, true).unwrap();
        assert!(dry.dry_run && dry.changed);
        assert_eq!(std::fs::read(&settings).unwrap(), before); // dry-run não escreve

        let real = apply_at(&settings, &exe, true, false).unwrap();
        assert!(real.status.installed);
        assert_eq!(real.status.other_hooks, 1);
        assert!(std::fs::read_to_string(&settings).unwrap().contains("orca-hook stop"));

        let un = apply_at(&settings, &exe, false, false).unwrap();
        assert!(!un.status.installed);
        assert!(std::fs::read_to_string(&settings).unwrap().contains("orca-hook stop"));
    }
}
