//! `nyx-hook install` / `uninstall`: CLI wrapper over `nyx_claude::hooks`.

use nyx_claude::hooks;
use std::path::PathBuf;

struct Opts {
    settings: PathBuf,
    dry_run: bool,
}

fn parse_opts(args: &[String]) -> Result<Opts, String> {
    let mut settings = None;
    let mut dry_run = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--dry-run" => dry_run = true,
            "--settings" => {
                settings = Some(PathBuf::from(it.next().ok_or("--settings precisa de um caminho")?))
            }
            other => return Err(format!("argumento desconhecido: {other}")),
        }
    }
    Ok(Opts { settings: settings.unwrap_or_else(hooks::default_settings_path), dry_run })
}

/// Returns the process exit code. Prints a short report to stdout.
pub fn run(args: &[String], install: bool) -> i32 {
    match run_inner(args, install) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("nyx-hook: {e}");
            1
        }
    }
}

fn run_inner(args: &[String], install: bool) -> Result<(), String> {
    let opts = parse_opts(args)?;
    // The CLI registers its own absolute path.
    let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    let exe = hooks::clean_path(&exe);
    let verb = if install { "install" } else { "uninstall" };
    let report = hooks::apply(&opts.settings, &exe, install, opts.dry_run).map_err(|e| format!("{e:#}"))?;

    if !report.changed {
        println!("{verb}: nada a mudar em {}", opts.settings.display());
    } else if report.dry_run {
        println!("{verb} (dry-run): {} seria alterado:\n{}", opts.settings.display(), report.diff);
    } else {
        if let Some(b) = &report.backup_path {
            println!("backup: {}", b.display());
        }
        println!("{verb}: {} atualizado\n{}", opts.settings.display(), report.diff);
    }
    Ok(())
}
