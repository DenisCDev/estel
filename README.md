# Estel

> *"Estel" é Sindarin para **esperança** — a calma de confiar que a noite passa, não a ansiedade de querer que algo aconteça.*

Estel é um ambiente ambient passivo, no estilo do f.lux, para Windows (e um irmão Android). Ao longo do dia ele esquenta a cor da tela, baixa o brilho e, se você quiser, coloca um ruído rosa/marrom bem baixo de noite.

**Não é um tratamento.** Não substitui acompanhamento clínico nem medicação. Os efeitos são reais e modestos: o ganho está em tirar estímulo que não precisava estar ali — tela branca-fria de madrugada, brilho no quarto escuro, som que começa seco.

---

## Instalar (Windows)

Precisa do [Rust](https://rustup.rs) (toolchain `stable`, alvo GNU ou MSVC) e, no GNU, do MinGW no `PATH` (`gcc`, `as`, `dlltool` — o MSYS2 em `C:\msys64\mingw64\bin` serve).

```powershell
git clone <url-deste-repo>
cd estel
.\install.ps1
```

O script compila em release, copia `estel.exe` para `%LOCALAPPDATA%\Estel` e inicia o app. O ícone aparece na bandeja.

Na bandeja:

- **Alta / Média / Suave** — quanto da curva entra (Suave é para jogo/filme)
- **Ruído noturno** — rosa de noite, marrom perto do sono; começa e termina em fade de 4 s; volume tem teto duro
- **Pausar** — devolve a tela agora, sem fechar
- **Configurações…** — acordar, dormir, volume, localização
- **Fechar Estel** — restaura gama e backlight

Primeira execução grava `%APPDATA%\condado\estel\config\config.toml`.

### Android

Abra `android/` no Android Studio, rode no aparelho. Na primeira abertura conceda a permissão de sobreposição — sem ela a camada quente não aparece. O serviço sobe sozinho se Estel estiver ativa.

---

## O que faz (e por quê)

Só o que a evidência segura: **temperatura de cor**, **menos azul de noite**, **menos brilho**, **som sem susto**.

| O quê | Por quê | Força |
|---|---|---|
| Curva de CCT (tipo f.lux), interpolada em mired, transições em smoothstep | Luz quente relaxa; luz fria em alta luminância aumenta excitação medida | Moderada |
| Cortar azul-ciano (~480 nm) de noite | ipRGC / melanopsina; consenso Brown et al. 2022 | Mecanismo forte |
| Baixar o brilho (DDC no monitor; overlay no notebook) | Tela brilhante no quarto escuro é re-adaptação constante | Moderada |
| Nunca piscar a UI, nunca dimmar por PWM de software | Flicker sub-visível aumenta cefaleia e FC em quem é sensível | Forte na minoria |
| Ruído rosa/marrom opcional, envelope ≥ 4 s, teto de volume | 1/f ajuda início de sono; ataque rápido é reflexo de sobressalto | Moderada / forte (ataque) |

A curva padrão (ajustável pelos horários de acordar/dormir):

| Fase | CCT | Brilho |
|---|---|---|
| Acordar | rampa → 6500 K | subindo |
| Dia | 6500 K | ~85–90 % |
| Início da noite | 6500 → 3400 K | caindo |
| Pré-sono | 3400 → 2700 K | baixo |
| Noite | 1900–2300 K | mínimo confortável |

Gama do Windows 11 recusa rampas agressivas em silêncio. Estel não tenta escurecer a tela por gama abaixo de ~50 %: o extra vai para DDC (monitor externo) ou para a sobreposição (notebook / HDR).

---

## O que foi deixado de fora

| Alegação | Evidência | Decisão |
|---|---|---|
| Óculos “bloqueadores de azul” | Provavelmente nulo (Cochrane 2023) | Não |
| Batidas binaurais | Misto, I²=91,6 % | Não |
| Matiz azul = calmante | Falha em replicar | Dessaturação importa, não o matiz |
| 432 Hz terapêutico | Fraco | Sem sino, sem alegação |
| Biometria / loop fechado | Fora do escopo + privacidade | Nunca |

Não tem prompt, não tem conta, não sai dado da máquina.

---

## Limites honestos

- Efeitos modestos. O maior ganho é **remover** o que ativa, não adicionar algo mágico.
- PWM de OLED (60–240 Hz) o software não muda — só evita o range de baixíssimo brilho.
- Variabilidade individual é alta. Tudo importante cabe na janela de configurações.
- **Não é tratamento.** Não substitui medicação nem acompanhamento clínico.

---

## Desenvolvimento

```powershell
# engine pura, sem janela
cargo test

# app
cargo run --release
```

O motor (`color`, `schedule`, `target`, `config`) não chama o sistema operacional. O host Windows aplica o `Target` em gama, DDC, overlay e áudio. O Android aplica o mesmo `Target` numa sobreposição.

Detalhes de API Win32: `docs/VERIFIED-DECISIONS.md`.

---

## Referências

- Brown TM et al. *PLOS Biology* 20(3):e3001571, 2022 — consenso melanópico EDI
- Singh S et al. *Cochrane Database of Systematic Reviews* 2023, Issue 8, CD013244 — óculos de luz azul
- Wilkins AJ et al. *Lighting Research & Technology* 21(1):11–18, 1989 — flicker e cefaleia
- Hazell & Wilkins. *Psychological Medicine*, 1990 — flicker e FC em agorafobia
- Wilms L & Oberfeld D. *Psychological Research*, 2018 — saturação > matiz
- Reutimann et al. *Royal Society Open Science* 10:230432, 2023 — cor e excitação em RV
- IEEE Std 1789-2015 — modulação de luz
- Blumenthal TD & Berg WK. *Psychophysiology*, 1986 — rise time e sobressalto
