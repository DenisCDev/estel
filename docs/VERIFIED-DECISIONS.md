# Estel — decisões técnicas verificadas

Fonte de verdade para o host Windows/Rust. Cada fato de API abaixo foi
checado contra docs atuais das crates / Microsoft Learn.

Regras de produto que prevalecem sobre o resto:

- Nunca mostrar pista médica, clínica, alarmante, urgente ou de “catástrofe”.
- Passivo / loop aberto: horário + localização. Sem biometria, sem prompt, sem coleta.
- Conforto adjuvante, não tratamento.
- Windows é o produto. Android é o irmão de overlay, mesmo motor.

---

## Display (gama + overlay + DDC)

- Crate `windows` **0.62.2**. `SetDeviceGammaRamp` / `GetDeviceGammaRamp` em
  `Win32::UI::ColorSystem`. `BOOL` é `windows::core::BOOL`.
- **Clamp silencioso do Win11:** cada entrada da rampa tem que ficar a no
  máximo 32768 da identidade; a chamada pode devolver TRUE sem aplicar.
  Estel clampa a rampa (`clamp_ramp_to_driver`) e **não dimma por gama abaixo
  de ~50 %**. Extra-dim = DDC ou overlay.
- HDR = gama no-op. Overlay cobre.
- A rampa é volátil (resolução, sleep, UAC). Reaplicar no tick.
- **Restore-on-next-launch:** arquivo `dirty` no diretório de config. Se o
  processo morreu, a próxima subida escreve identidade *antes* do snapshot.
- Overlay: `WS_EX_LAYERED | TRANSPARENT | NOACTIVATE | TOOLWINDOW`, PeekMessage
  filtrado no HWND do overlay. `WM_DISPLAYCHANGE` redimensiona.
- DDC: `SetMonitorBrightness`. Restore é idempotente (`DestroyPhysicalMonitor`
  uma vez). `park()` devolve o backlight sem soltar o handle (Pausar).

## CCT e curva

- Tanner Helland, sem crate. Interpolação de CCT em **mired**. Smoothstep em
  toda rampa. Engine pura, testável, sem chamada de OS.

## Áudio

- `rodio` 0.22 (`DeviceSinkBuilder` + `Player`). Sem chime: um tom na virada
  de fase é sobressalto.
- `set_volume(0)` **antes** de `append`. Fade de 4 s. Teto duro (`HARD_CAP`)
  depois do `max_volume` do usuário.

## Tray / UI / autostart

- `tray-icon` 0.24 + `muda` 0.19. `eframe` 0.32 na janela de configurações
  (thread própria, um único exemplar).
- Single instance: `CreateMutexW` + `Local\\EstelSingleInstance`.
- Autostart: `auto-launch` HKCU, sem UAC.
- `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]`.
  Log em `estel.log` no diretório de config.

## Config

- `directories` 6 + `serde` + `toml` 1.1. TOML inválido vira
  `config.toml.invalid`; memória cai no padrão. `sanitize()` clampa volume,
  tick, lat/lon e keypoints vazios.
