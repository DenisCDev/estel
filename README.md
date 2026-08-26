<h1 align="center">Estel</h1>

<p align="center">
  <b>Aplicativo para Windows e Android que ajusta a cor e o brilho da tela ao longo do dia</b>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/windows-rust-D4A24E?labelColor=171310" alt="aplicativo Windows em Rust">
  <img src="https://img.shields.io/badge/android-kotlin-D4A24E?labelColor=171310" alt="aplicativo Android em Kotlin">
  <img src="https://img.shields.io/badge/dados-locais-43A48E?labelColor=171310" alt="dados mantidos localmente">
  <img src="https://img.shields.io/badge/license-MIT-D4A24E?labelColor=171310" alt="licença MIT">
</p>

<p align="center">
  <img src="assets/mtg-phial.jpg" width="640" alt="Frasco de Galadriel — arte de Andrea Piparo, Tales of Middle-earth (2023)">
</p>

<p align="center">
  <sub><i>"a light to you in dark places"</i><br>
  — <b>A Sociedade do Anel</b>, livro II, capítulo VIII · arte de Andrea Piparo para Magic: The Gathering, Tales of Middle-earth (2023)
</p>

O Estel muda gradualmente a temperatura de cor e o brilho da tela no Windows e
no Android. A versão Windows também pode tocar ruído rosa ou marrom em volume
baixo durante a noite. Tudo roda no aparelho, sem conta e sem coleta de dados
biométricos.

**Não é um tratamento médico** e não substitui acompanhamento profissional ou
medicação. As referências no fim do documento explicam as decisões de produto;
os efeitos variam de pessoa para pessoa.

---

## Instalar (Windows)

Precisa do [Rust](https://rustup.rs) (toolchain `stable`, alvo GNU ou MSVC) e, no GNU, do MinGW no `PATH` (`gcc`, `as`, `dlltool` — o MSYS2 em `C:\msys64\mingw64\bin` serve).

```powershell
git clone https://github.com/DenisCDev/estel.git
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

Abra `android/` no Android Studio e rode no aparelho. Na primeira abertura,
conceda a permissão de sobreposição — sem ela a camada quente não aparece. O
serviço aplica apenas os ajustes visuais; o som ambiente está disponível na
versão Windows.

---

## O que faz e por quê

| Ajuste | Objetivo | Como funciona |
|---|---|---|
| Temperatura de cor | Reduzir luz azul-ciano no período noturno | Curva gradual em mired, com transições suaves |
| Brilho | Evitar uma tela muito clara em um ambiente escuro | DDC no monitor ou camada escura no notebook |
| Estabilidade visual | Evitar mudanças bruscas e cintilação criada pelo aplicativo | Sem piscar a interface nem simular PWM por software |
| Som opcional no Windows | Criar um fundo constante sem início ou fim abrupto | Ruído rosa ou marrom, transição de pelo menos 4 s e limite de volume |

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
