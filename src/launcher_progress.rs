//! A driver-independent status window while a manual launch is slow or recovering.

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{HSTRING, w};

pub(crate) struct Progress {
    window: Option<HWND>,
    interactive: bool,
}

impl Default for Progress {
    fn default() -> Self {
        Self {
            window: None,
            interactive: !std::env::args_os().any(|arg| arg == "--startup"),
        }
    }
}

impl Progress {
    pub(crate) fn show(&mut self, attempt: u32) -> anyhow::Result<()> {
        if self.window.is_some() || !self.interactive {
            return Ok(());
        }
        let window = unsafe {
            CreateWindowExW(
                WS_EX_APPWINDOW,
                w!("STATIC"),
                w!("Estel · inicialização"),
                WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_VISIBLE,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                520,
                150,
                None,
                None,
                None,
                None,
            )?
        };
        self.window = Some(window);
        let text = if attempt == 0 {
            "O Estel está iniciando. Aguarde a abertura.\nVocê pode fechar esta janela para cancelar.".into()
        } else {
            format!(
                "O Estel encontrou uma falha e está tentando abrir novamente.\nTentativa de recuperação {attempt} de 3.\nVocê pode fechar esta janela para cancelar."
            )
        };
        unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("STATIC"),
                &HSTRING::from(text),
                WS_CHILD | WS_VISIBLE,
                16,
                16,
                480,
                90,
                Some(window),
                None,
                None,
                None,
            )?
        };
        // Autorun can inherit SW_HIDE; a later manual request must still show feedback.
        unsafe {
            SetWindowPos(
                window,
                None,
                0,
                0,
                0,
                0,
                SWP_SHOWWINDOW | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER,
            )?
        };
        tracing::debug!(attempt, "aviso de inicialização exibido");
        Ok(())
    }

    pub(crate) fn cancelled(&self) -> bool {
        self.window
            .is_some_and(|window| !unsafe { IsWindow(Some(window)) }.as_bool())
    }

    pub(crate) fn hide(&mut self) {
        if let Some(window) = self.window.take()
            && unsafe { IsWindow(Some(window)) }.as_bool()
            && let Err(error) = unsafe { DestroyWindow(window) }
        {
            tracing::warn!(%error, "não foi possível fechar o aviso de inicialização");
        }
    }

    pub(crate) fn enable(&mut self) {
        self.interactive = true;
    }

    pub(crate) fn interactive(&self) -> bool {
        self.interactive
    }
}

impl Drop for Progress {
    fn drop(&mut self) {
        self.hide();
    }
}
