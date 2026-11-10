//! Job Object do Windows com `KILL_ON_JOB_CLOSE`: cada `claude` das delegações (e os
//! filhos que ele criar depois de entrar) fica num job aberto pela Nyx. Quando a Nyx
//! sai por qualquer motivo, inclusive crash, o SO fecha o handle e mata a árvore.
//! O `kill_all` no `RunEvent::Exit` continua como reforço.

use std::process::Child;
use std::sync::OnceLock;

#[cfg(windows)]
mod imp {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    pub struct Job(HANDLE);

    // O handle de job pode ser usado de qualquer thread.
    unsafe impl Send for Job {}
    unsafe impl Sync for Job {}

    impl Job {
        pub fn new() -> Option<Job> {
            // SAFETY: chamadas Win32 com ponteiros nulos/struct zerada válidos; o handle
            // é conferido e fechado no Drop.
            unsafe {
                let h = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if h.is_null() {
                    return None;
                }
                let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                let ok = SetInformationJobObject(
                    h,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const core::ffi::c_void,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                );
                if ok == 0 {
                    CloseHandle(h);
                    return None;
                }
                Some(Job(h))
            }
        }

        /// Põe o próprio processo no job: tudo que ele criar depois herda o job.
        pub fn assign_current(&self) -> bool {
            // SAFETY: GetCurrentProcess devolve um pseudo-handle válido, sem fechar.
            unsafe {
                AssignProcessToJobObject(self.0, windows_sys::Win32::System::Threading::GetCurrentProcess()) != 0
            }
        }

        pub fn assign(&self, child: &std::process::Child) -> bool {
            // SAFETY: o handle do processo pertence ao `Child` vivo durante a chamada.
            unsafe { AssignProcessToJobObject(self.0, child.as_raw_handle() as HANDLE) != 0 }
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            // SAFETY: handle criado por nós e fechado uma vez.
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub struct Job;
    impl Job {
        pub fn new() -> Option<Job> {
            None
        }
        pub fn assign(&self, _: &std::process::Child) -> bool {
            false
        }
        pub fn assign_current(&self) -> bool {
            false
        }
    }
}

pub use imp::Job;

static GLOBAL: OnceLock<Option<Job>> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobMode {
    /// O Nyx inteiro está no job: todo filho (claude, netos, WebView2) herda o job.
    WholeApp,
    /// O Windows recusou pôr o Nyx no job (ex.: já está num job sem breakaway, como no
    /// terminal do VS Code): cada `claude` é atribuído logo após o spawn.
    PerChild,
    Unavailable,
}

/// Cria o job do app (no start) e tenta pôr o próprio Nyx nele. O job nunca é fechado
/// explicitamente: fecha com o processo e leva a árvore junto.
/// Atenção: qualquer app que o Nyx lançar como filho morre quando o Nyx sai.
pub fn init() -> JobMode {
    match GLOBAL.get_or_init(Job::new) {
        Some(j) if j.assign_current() => JobMode::WholeApp,
        Some(_) => JobMode::PerChild,
        None => JobMode::Unavailable,
    }
}

/// Põe o processo no job do app (cria o job se o `init` ainda não rodou).
pub fn assign(child: &Child) -> bool {
    match GLOBAL.get_or_init(Job::new) {
        Some(j) => j.assign(child),
        None => false,
    }
}
