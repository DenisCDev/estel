# Estel — decisões técnicas verificadas

Fonte de verdade para o host Windows/Rust. Cada fato de API abaixo foi
checado contra docs atuais das crates / Microsoft Learn.

Regras de produto que prevalecem sobre o resto:

- Conforto visual é personalizável; nenhum ajuste de cor ou brilho é apresentado
  como tratamento para ansiedade, depressão ou pânico.
- Ajustes automáticos usam horário, localização e entradas opcionais de clima ou
  câmera; não há biometria, análise de emoção nem coleta de imagens.
- Exercícios voluntários e contatos humanos por país ficam nos ajustes. As
  fontes e seus limites estão em [COMFORT-EVIDENCE.md](COMFORT-EVIDENCE.md).
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
  processo morreu enquanto ajustava a tela, a próxima subida escreve identidade
  *antes* do snapshot. O modo de cores preservadas também recupera uma sessão
  anterior incompleta antes de manter a tela sem ajustes.
- Overlay: `WS_EX_LAYERED | TRANSPARENT | NOACTIVATE | TOOLWINDOW`, PeekMessage
  filtrado no HWND do overlay. `WM_DISPLAYCHANGE` redimensiona.
- DDC: `SetMonitorBrightness`. Restore é idempotente (`DestroyPhysicalMonitor`
  uma vez). `park()` devolve o backlight sem soltar o handle (Pausar).
- Em várias telas, DDC e gama só aplicam o alvo quando todas as saídas
  detectadas aceitam o ajuste. Uma falha parcial ou mudança de conexão
  desativa o caminho físico até o próximo início; a sobreposição cobre todo o
  desktop virtual. A restauração dos monitores afetados é repetida enquanto
  houver falha transitória. Isso sincroniza o ajuste enviado pelo Estel, mas
  não substitui a calibração de fábrica/OSD de cada painel.

## CCT e curva

- Tanner Helland, sem crate. Interpolação de CCT em **mired**. Smoothstep em
  toda rampa. Engine pura, testável, sem chamada de OS.

## Trabalho com cores e daltonismo

- A [International Color Consortium](https://www.color.org/displaycalibration/)
  explica que calibração e perfil do monitor são usados para reprodução
  consistente de cores. Ela também registra que alterações na iluminação do
  ambiente podem reduzir a precisão. Por isso, **Trabalho com cores** restaura
  a gama e o brilho capturados antes do Estel e suspende a sobreposição.
- O [National Eye Institute](https://www.nei.nih.gov/eye-health-information/eye-conditions-and-diseases/color-blindness)
  descreve diferentes tipos de deficiência de visão de cores e afirma que não
  existe cura para a forma hereditária. Sem conhecer o tipo e as necessidades
  da pessoa, o Estel não aplica uma transformação global que alegue corrigir
  daltonismo. **Tenho daltonismo** preserva a imagem original.
- Um [estudo experimental publicado em Scientific Reports (2022)](https://pubmed.ncbi.nlm.nih.gov/35778454/)
  não encontrou melhora significativa de discriminação de cores com os filtros
  ópticos avaliados em participantes com deficiência de visão vermelho-verde.
  O estudo testou óculos, não filtros digitais; ele reforça a cautela, mas não
  demonstra que todo recurso digital seja ineficaz.
- A [W3C WCAG 2.2, critério 1.4.1](https://www.w3.org/WAI/WCAG22/Understanding/use-of-color)
  recomenda não usar cor como único sinal de informação. Os botões do Estel
  mostram o estado por texto (ligado/desligado, sim/não), além da cor.
- As duas escolhas são independentes e persistentes. Enquanto uma delas estiver
  ativa, os ajustes de tela do Estel, inclusive brilho por câmera, ficam em
  pausa. Som e configuração continuam. Desmarcar ambas retoma os ajustes.

## Atualização opcional

- A [API oficial de releases do GitHub](https://docs.github.com/en/rest/releases/releases)
  informa a última versão publicada e os ativos. Um commit no repositório, sem
  release, não é oferecido como atualização.
- O [campo SHA-256 do ativo](https://docs.github.com/en/rest/releases/assets)
  é comparado ao instalador baixado, junto com nome, origem e tamanho. O Estel
  só abre o assistente após o clique do usuário e a verificação completa.

## Áudio

- `rodio` 0.22 (`DeviceSinkBuilder` + `Player`). Sem chime: um tom na virada
  de fase é sobressalto.
- `set_volume(0)` **antes** de `append`. Fade de 4 s; a troca de cor passa por
  silêncio. `HARD_CAP` limita o ganho digital após `max_volume`, sem medir dBA.

## Tray / UI / autostart

- `tray-icon` 0.24 + `muda` 0.19. `eframe` 0.32 na janela de configurações
  (thread própria, um único exemplar).
- Single instance: `CreateMutexW` + `Local\\EstelSingleInstance`.
- Autostart: o instalador cria a entrada HKCU na primeira instalação; `auto-launch`
  permite alterar a opção pela bandeja, sem UAC.
- `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]`.
  Log em `estel.log` no diretório de config.

## Config

- `directories` 6 + `serde` + `toml` 1.1. TOML inválido vira
  `config.toml.invalid`; memória cai no padrão. `sanitize()` clampa volume,
  tick, lat/lon e keypoints vazios.

## Luz ambiente por câmera

- Media Foundation enumera câmeras e lê quadros YUY2. Depois de pelo menos
  500 ms, verifica cinco quadros com variação máxima de 0,05 na média normalizada,
  sem valores fora de 0,02–0,98. Até 90 tentativas de leitura e prazo externo de 5 s.
- A captura roda em um processo auxiliar com limite de 5 s, acionado por uma
  thread dedicada. Em novas configurações, a opção vem desligada. Quando desligada
  ou sem referências aceitas para a câmera escolhida, a thread espera nova
  configuração e não abre a câmera. Uma leitura válida
  calcula no máximo 8.000 amostras por quadro, descarta os quadros e só envia um `f32` de
  brilho ao loop principal.
- A câmera tem prioridade sobre a estimativa de clima, mas corrige apenas 35%
  da diferença para a curva por horário. Sua leitura é limitada pela configuração
  da pessoa e suavizada por EWMA (20%).
  Referências relativas são aceitas somente com contraste de pelo menos 0,10,
  mesma identidade de dispositivo e sinal dentro da faixa. Esses limiares são
  escolhas de engenharia, não calibração fotométrica. Falhas descartam a leitura
  e devolvem o cálculo ao horário/clima opcional com transição gradual. Uma
  recalibração só substitui referências salvas após o novo par ser válido.
  A câmera não é usada como luxímetro calibrado, biometria, detector de rosto,
  olhos, presença, emoção ou atenção.
