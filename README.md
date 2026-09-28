<h1 align="center">Estel</h1>

<p align="center">
  <b>Aplicativo para Windows e Android que ajusta a cor e o brilho da tela ao longo do dia</b>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/windows-rust-D4A24E?labelColor=171310" alt="aplicativo Windows em Rust">
  <img src="https://img.shields.io/badge/android-kotlin-D4A24E?labelColor=171310" alt="aplicativo Android em Kotlin">
  <img src="https://img.shields.io/badge/câmera-local-43A48E?labelColor=171310" alt="quadros da câmera mantidos localmente">
  <img src="https://img.shields.io/badge/license-MIT-D4A24E?labelColor=171310" alt="licença MIT">
</p>

<p align="center">
  <img src="assets/avatar-icon.png" width="180" alt="Avatar do Estel: personagem de cabelo preto com mechas verdes">
</p>

<p align="center">
  <sub>Avatar inspirado no personagem do <a href="https://github.com/DenisCDev/portfolio-site">portfólio DenisDev</a>.</sub>
</p>

A interface Windows usa as fontes Fredoka e Lilita One, distribuídas sob a
SIL Open Font License nos arquivos `assets/fredoka-OFL.txt` e
`assets/lilitaone-OFL.txt`.

O Estel muda gradualmente a temperatura de cor e o brilho da tela no Windows e
no Android. No Windows, a pessoa pode escolher ajustes suaves por clima/janela
ou câmera e abrir uma pausa visual ou um exercício de aterramento. A versão
Windows também pode tocar ruído rosa ou marrom em volume
baixo durante a noite. Tudo roda no aparelho, sem conta e sem coleta de dados
biométricos. A busca de cidade e a consulta de clima são recursos online descritos abaixo.

As escolhas de conforto visual variam de pessoa para pessoa. A seção de apoio
oferece contatos humanos por país; as [evidências e limites](docs/COMFORT-EVIDENCE.md)
explicam o que foi estudado e o que ainda é uma escolha de engenharia.

---

## Instalar (Windows)

Não precisa instalar Git, Rust nem abrir o terminal.

1. [Baixe o instalador do Estel para Windows](https://github.com/DenisCDev/estel/releases/latest/download/Estel-Setup-x86_64.exe).
2. Abra `Estel-Setup-x86_64.exe` e avance pelo instalador.
3. No fim, deixe **Abrir Estel** marcado. As configurações abrem e o ícone fica
   ao lado do relógio ou dentro da seta **Mostrar ícones ocultos**.

O instalador funciona por usuário, sem pedir senha de administrador. Ele cria um
atalho no menu Iniciar e ativa **Iniciar com o Windows** na primeira instalação;
essa opção pode ser desligada no menu da bandeja. O aplicativo pode ser removido
pelas Configurações do Windows. Como
o aplicativo ainda não tem assinatura digital, o Windows pode mostrar o
SmartScreen: clique em **Mais informações** e depois em **Executar assim mesmo**.

Quem não quiser instalar pode baixar o
[`estel-portable-x86_64.exe`](https://github.com/DenisCDev/estel/releases/latest/download/estel-portable-x86_64.exe)
e abri-lo diretamente. O portátil guarda as configurações no mesmo local da
versão instalada.

Na bandeja:

- **Alta / Média / Suave** — força da curva; Média é o padrão para novas instalações
- **Ruído noturno opcional** — rosa ou marrom, com transições suaves e limite digital de ganho; o volume real depende do dispositivo
- **Pausar** — devolve a tela agora, sem fechar
- **Configurações…** — acordar, dormir, luz, pausa visual e apoio emocional
- **Buscar atualização** — abre os ajustes e verifica a versão publicada no GitHub
- **Fechar Estel** — restaura gama e backlight

Primeira execução grava `%APPDATA%\condado\estel\config\config.toml`.

### Atualizar ou remover

O Estel verifica se há uma versão publicada mais recente ao iniciar e mostra o
aviso no menu da bandeja. Nos ajustes, **Atualizar agora** baixa o instalador,
confere tamanho e SHA-256 e abre o assistente do Windows. A instalação só começa
quando você avança no assistente e preserva suas configurações. Para remover,
abra **Configurações do Windows → Aplicativos → Aplicativos instalados**, procure
por **Estel** e escolha **Desinstalar**.

### Cores fiéis e daltonismo

Nos ajustes, marque **Trabalho com cores** ou **Tenho daltonismo** para preservar
as cores originais do monitor. Enquanto qualquer uma dessas opções estiver
ligada, o Estel pausa os ajustes de cor e brilho da tela; o som opcional pode
continuar. Não há diagnóstico nem filtro universal para daltonismo. As fontes e
os limites dessa escolha estão em [decisões verificadas](docs/VERIFIED-DECISIONS.md).

Se a câmera falhar depois de medir a luz, o Estel mantém a última leitura por
até cinco minutos. Depois volta gradualmente ao brilho calculado pelo horário;
mudanças automáticas de brilho são limitadas a 6 pontos percentuais por ajuste.
Mudanças automáticas de cor também são graduais. Alterar apenas o intervalo de
leitura da câmera não descarta a medição atual. O menu da bandeja informa quando
a última leitura está sendo mantida. Se outro aplicativo estiver usando a
câmera, libere-a para tentar novamente; o log do Estel registra uma orientação
para essa falha.

Com duas telas, o Estel só usa o controle físico de brilho e a gama quando
todas aceitam o ajuste. Se uma não responder ou a conexão dos monitores mudar,
uma sobreposição comum mantém os ajustes sincronizados nessa sessão. Brilho e
cores percebidos ainda dependem da calibração própria de cada monitor.
Se uma sessão antiga terminou abruptamente com vários monitores, o registro
anterior não identifica cada tela. O Estel preserva esse registro e usa a
sobreposição, sem arriscar restaurar o brilho físico na tela errada.

### Android

Abra `android/` no Android Studio e rode no aparelho. Na primeira abertura,
conceda a permissão de sobreposição — sem ela a camada quente não aparece. O
serviço aplica apenas os ajustes visuais; o som ambiente está disponível na
versão Windows.

---

## O que faz e por quê

| Ajuste | Objetivo | Como funciona |
|---|---|---|
| Temperatura de cor | Ajustar a aparência da tela ao longo do dia conforme preferência | Curva gradual em mired, com transições suaves |
| Brilho | Evitar uma tela desconfortavelmente clara ou escura | DDC no monitor ou camada escura no notebook; ajuste pessoal continua importante |
| Luz ambiente no Windows | Corrigir suavemente o brilho de base pela claridade aproximada | Desligada em novas configurações; quando ativada, a câmera mede a média de um quadro, descarta-o localmente e corrige 35% da diferença para a curva por horário |
| Clima e janela no Windows | Estimar a claridade quando a câmera não mede | Consulta opcional de radiação solar e posição aproximada do sol ajustam o brilho por horário; a janela pode ser configurada como de frente, de costas ou de lado para a tela |
| Estabilidade visual | Evitar mudanças bruscas e cintilação criada pelo aplicativo | Sem piscar a interface nem simular PWM por software |
| Som opcional no Windows | Oferecer um fundo sonoro para quem o prefere | Ruído rosa ou marrom, troca com saída e entrada graduais; o limite digital não mede o volume nos ouvidos |
| Pausa visual e aterramento no Windows | Oferecer ações simples quando a pessoa quiser | Pausa visual de 20 s e exercício de atenção aos sentidos; contatos de apoio por país |

A curva base na intensidade Alta (Média e Suave reduzem o efeito):

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
| Batidas binaurais | Sem demonstração de benefício para este aplicativo | Não |
| Cor azul como tratamento calmante | Estudos de cor e emoção dependem do contexto e não testam o Estel | Sem promessa terapêutica por matiz |
| 432 Hz terapêutico | Fraco | Sem sino, sem alegação |
| Biometria / loop fechado | Fora do escopo + privacidade | Nunca |

Não tem conta nem coleta de imagens. As consultas online opcionais enviam a busca
digitada ou as coordenadas ao Open-Meteo; os quadros da câmera permanecem locais.

### Luz ambiente por câmera (Windows)

O ajuste por câmera vem desligado em novas instalações. Quem quiser pode ativar
a opção e escolher a webcam nas configurações. O Estel abre a câmera apenas
para obter um quadro, calcula
a luminância média de
até 8.000 pixels, descarta o quadro em memória e fecha o acesso. A leitura
padrão acontece a cada 30 segundos, tem limite de 5 segundos e o resultado é
suavizado antes de alterar o brilho.

Se a câmera estiver indisponível, o brilho segue a curva por horário com uma
correção limitada pela radiação solar, se a consulta de clima estiver ativa.
Mudanças feitas na janela de configurações são aplicadas assim que são salvas.

Não há gravação, visualização ou transmissão dos quadros, identificação de pessoas, rosto, olhos,
presença ou estado emocional. Uma webcam comum não é um luxímetro: exposição
automática e posição da câmera mudam a leitura. Por isso o recurso trabalha
com um sinal relativo e deixa a pessoa definir os limites para ambiente escuro
e claro. Com a câmera ativa, o brilho estimado corrige parcialmente a curva por
horário, sem substituir o limite noturno por uma leitura de 100%. A câmera tem
prioridade sobre a estimativa de clima; desativar a opção restaura a curva com
clima opcional e não abre a câmera. Se o Windows tiver um sensor de luz ambiente,
experimente o brilho automático do próprio sistema primeiro e evite dois
controladores automáticos ao mesmo tempo.

### Localização, clima e orientação (Windows)

Na primeira instalação, a janela de configurações solicita a localização ao
Windows. Se o acesso for negado, busque cidade ou bairro pelo botão **Buscar**,
escolha o resultado e confira o ponto no mapa. A busca é feita pelo serviço de
geocodificação do [Open-Meteo](https://open-meteo.com/en/docs/geocoding-api),
com dados do GeoNames. Bairros sem cadastro podem não aparecer; nesse caso,
ajuste latitude e longitude manualmente após conferir o ponto no mapa.

O clima vem do [Open-Meteo](https://open-meteo.com/en/docs) e é consultado no
máximo a cada 15 minutos, com as coordenadas escolhidas. Quando a rede falha,
o Estel volta ao brilho por horário. O serviço gratuito é destinado a
[uso não comercial](https://open-meteo.com/en/terms); desative **Usar clima**
se essa condição não se aplicar. O serviço disponibiliza os dados sob CC BY 4.0.

Se há uma janela perto, informe sua orientação pela bússola do celular e se a
tela fica de frente, de costas ou de lado para ela. O cálculo usa a posição
aproximada do sol e a radiação direta para compensar reflexos, até um limite
pequeno. A câmera, quando funciona, sempre tem prioridade sobre essa estimativa.
Nascer e pôr do sol da localização continuam definindo a curva de cor.

A janela de configurações usa quatro cenas ilustradas derivadas do mascote
original do [portfólio do Denis](https://github.com/DenisCDev/portfolio-site),
preservando a identidade descrita em `astro/MASCOTES.md` naquele repositório.

---

## Limites honestos

- Efeitos modestos. O maior ganho é **remover** o que ativa, não adicionar algo mágico.
- O software não muda a modulação elétrica do painel nem mede sua cintilação.
- Variabilidade individual é alta. Tudo importante cabe na janela de configurações.
- A leitura ambiental é uma ajuda ergonômica, não um diagnóstico de fadiga,
  ansiedade ou saúde ocular. Pausas, iluminação difusa e redução de reflexos
  continuam sendo importantes.
- Ruído rosa ou marrom não tem benefício geral comprovado para foco, ansiedade
  ou sono. O ganho digital não garante um nível seguro em decibéis nos fones.
- O exercício de aterramento e os contatos de apoio são iniciados pela pessoa;
  o aplicativo não detecta nem trata crises.

---

## Desenvolvimento

```powershell
# precisa de Rust apenas para desenvolver ou compilar o projeto
# engine pura, sem janela
cargo test

# app
cargo run --release
```

O `install.ps1` também é voltado a desenvolvimento: compila o código local e
instala esse build em `%LOCALAPPDATA%\Estel`. Para uso normal, baixe o instalador
pronto na seção acima.

O motor (`color`, `schedule`, `target`, `config`) não chama o sistema operacional. O host Windows aplica o `Target` em gama, DDC, overlay e áudio. O Android aplica o mesmo `Target` numa sobreposição.

Detalhes de API Win32: `docs/VERIFIED-DECISIONS.md`.

---

## Referências

- Brown TM et al. *PLOS Biology* 20(3):e3001571, 2022 — consenso melanópico EDI
- Singh S et al. *Cochrane Database of Systematic Reviews* 2023, Issue 8, CD013244 — óculos de luz azul
- Wilkins AJ et al. *Lighting Research & Technology* 21(1):11–18, 1989 — flicker e cefaleia
- Hazell & Wilkins. *Psychological Medicine*, 1990 — flicker e FC em agorafobia
- Wilms L & Oberfeld D. *Psychological Research*, 2018 — brilho, saturação e emoção em estímulos controlados
- Reutimann et al. *Royal Society Open Science* 10:230432, 2023 — cor e excitação em RV
- IEEE Std 1789-2015 — modulação de luz
- Blumenthal TD & Berg WK. *Psychophysiology*, 1986 — rise time e sobressalto
- Sheedy JE et al. *Ergonomics* 48(9):1114–1128, 2005,
  [doi:10.1080/00140130500208414](https://doi.org/10.1080/00140130500208414)
  — luminância ao redor da tela e adaptação visual
- [ISO/TR 9241-610:2022](https://www.iso.org/obp/ui/en/#iso:std:iso:tr:9241:-610:ed-1:v1:en)
  — impacto da luz e da iluminação em sistemas interativos
- [OSHA — iluminação em estações de computador](https://www.osha.gov/etools/computer-workstations/workstation-environment)
  — reflexos, contraste e fadiga visual
