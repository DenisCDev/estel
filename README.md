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
3. No fim, deixe **Configurar e abrir Estel** marcado. As configurações abrem e o ícone fica
   ao lado do relógio ou dentro da seta **Mostrar ícones ocultos**.

A partir da versão 0.3.1, uma instalação nova abre um guia com localização,
janela, câmera ou sensor de luz, rotina e limites de brilho. Os ajustes de tela e
som aguardam a conclusão do guia. Localização do Windows e luz ambiente são
opcionais; você pode usar coordenadas, buscar uma cidade ou manter a referência
inicial de São Paulo e revisar depois. A câmera só captura quando você escolhe
usar luz ambiente e solicita referências, ou quando já há referências válidas
para o ajuste automático.

Para a direção da janela, use a bússola do celular: aponte o topo do aparelho do
interior do cômodo para fora da janela e leia os graus, longe de ímãs e objetos
metálicos. Se houver essa opção na bússola, use norte verdadeiro (geográfico),
a referência da posição do sol. Norte é 0°, leste 90°, sul 180° e oeste 270°. É uma estimativa da
orientação da janela, não das coordenadas da cidade; deixar a direção indefinida
também é válido. O guia explica a posição da janela em relação à tela.

Atualizações mantêm as preferências existentes. Para repetir as perguntas,
abra o painel e selecione **Configuração guiada · local, janela e câmera**.
Se fechar antes de concluir uma configuração nova, suas escolhas ficam salvas
e o guia reaparece na próxima abertura.

O instalador funciona por usuário, sem pedir senha de administrador. Ele cria um
atalho no menu Iniciar e ativa **Iniciar com o Windows** na primeira instalação;
essa opção pode ser desligada no menu da bandeja. O aplicativo pode ser removido
pelas Configurações do Windows. O aplicativo ainda não tem assinatura digital;
o SmartScreen pode avisar e o Smart App Control pode impedir a execução.
A preparação da publicação assinada está em [assinatura no Windows](docs/WINDOWS-SIGNING.md).

Quem não quiser instalar pode baixar o
[`estel-portable-x86_64.exe`](https://github.com/DenisCDev/estel/releases/latest/download/estel-portable-x86_64.exe)
e abri-lo diretamente. O portátil guarda as configurações no mesmo local da
versão instalada.

Na bandeja:

- **Alta / Média / Suave** — força da cor e do som, sem alterar o brilho confortável; Média é o padrão para novas instalações
- **Ruído noturno opcional** — rosa ou marrom, com transições suaves e limite digital de ganho; o volume real depende do dispositivo
- **Pausar** — devolve a tela agora, sem fechar
- **Configurações…** — acordar, dormir, luz, pausa visual e apoio emocional
- **Buscar atualização** — abre os ajustes e verifica a versão publicada no GitHub
- **Fechar Estel** — restaura gama e backlight

Primeira execução grava `%APPDATA%\condado\estel\config\config.toml`.
As gravações substituem o arquivo inteiro apenas depois de concluídas e mantêm
as preferências anteriores em `config.toml.bak`. Se o arquivo principal ficar
danificado, o Estel recupera esse backup e preserva o conteúdo danificado em
`config.toml.invalid`; sem backup válido, avisa em vez de gravar padrões.
Alterações pelo painel e pela bandeja preservam os campos que você não editou.
A preferência de iniciar com o Windows também fica salva para reparar um
registro removido ou com caminho antigo na próxima abertura do aplicativo.

No Windows 0.3.2, um processo separado acompanha a inicialização e a execução
do Estel. A inicialização tem prazo de 45 segundos; se o processo principal
falhar, tenta reabri-lo até três vezes, com espera crescente. **Fechar Estel**,
encerrar a sessão e o comando `--quit` encerram normalmente, sem reiniciar.
Falhas do painel ou de sensores opcionais não fecham o aplicativo principal.
O registro do instalador inclui `--startup`, como o registro feito pelo aplicativo.

Os diagnósticos ficam na pasta de configuração: `launcher.log` registra as
tentativas de abertura, `estel.log` registra o processo principal e os demais
processos têm logs separados (`settings.log`, `display.log`, `camera.log`,
`light-sensor.log` e `diagnostics.log`). Cada arquivo guarda até 2 MiB e um backup
`.log.1`, incluindo falhas anteriores à criação da bandeja. Se as tentativas de
recuperação se esgotarem, uma mensagem orienta como encontrar os registros.
O início automático depende de o Windows executar o registro ao entrar na
sessão; ele pode adiar essa execução. A recuperação começa quando o iniciador
é executado, e não substitui bloqueios de segurança ou uma opção de início
automático desativada no Windows.

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

Se a câmera falhar ou sua leitura for rejeitada, o Estel descarta essa leitura
e volta gradualmente ao brilho calculado pelo horário e pelo clima opcional;
mudanças automáticas de brilho são limitadas a 6 pontos percentuais por ajuste.
Mudanças automáticas de cor também são graduais. Alterar apenas o intervalo de
leitura da câmera não descarta a medição atual. O menu da bandeja informa quando
o ajuste voltou ao horário. Se outro aplicativo estiver usando a
câmera, libere-a para tentar novamente; o log do Estel registra uma orientação
para essa falha.

Cada tela escolhe seu próprio ajuste: brilho físico por DDC/CI ou WMI quando
disponível, gama somente em SDR confirmado e sobreposição nos demais casos.
Conectar ou trocar uma tela dispara nova identificação. HDR e gerenciamento
avançado de cor permanecem sob controle do Windows. A redução de brilho por
sobreposição é preta; o aquecimento por sobreposição é aproximado e pode elevar
as sombras. Ele não equivale a uma transformação de cor HDR.

O Estel salva brilho e gama originais por identidade antes de alterá-los. Se um
monitor estiver ausente na restauração, mantém o registro para tentar novamente.
Sessões antigas que nunca salvaram a gama original não permitem recuperar essa
calibração: o painel informa a pendência e esse ajuste fica desabilitado.
Tecnologia do painel só é mostrada quando o monitor a informa; OLED, IPS, VA,
mini-LED ou cintilação não são inferidos pelo nome comercial.
As decisões, alternativas e fontes estão em [adaptação ao hardware](docs/WINDOWS-HARDWARE.md).

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
| Luz ambiente no Windows | Corrigir suavemente o brilho de base pela claridade aproximada | Desligada em novas configurações; exige duas referências relativas, confere cinco quadros estáveis e corrige 35% da diferença para a curva por horário dentro dos limites pessoais |
| Clima e janela no Windows | Estimar a claridade quando a câmera não mede | Consulta opcional de radiação solar e posição aproximada do sol ajustam o brilho por horário; a janela pode ser configurada como de frente, de costas ou de lado para a tela |
| Estabilidade visual | Evitar mudanças bruscas e cintilação criada pelo aplicativo | Sem piscar a interface nem simular PWM por software |
| Som opcional no Windows | Oferecer um fundo sonoro para quem o prefere | Ruído rosa ou marrom, troca com saída e entrada graduais; o limite digital não mede o volume nos ouvidos |
| Pausa visual e aterramento no Windows | Oferecer ações simples quando a pessoa quiser | Pausa visual de 20 s e exercício de atenção aos sentidos; contatos de apoio por país |

A curva de cor na intensidade Alta (Média e Suave reduzem cor e som, sem elevar brilho):

| Fase | CCT | Brilho |
|---|---|---|
| Acordar | rampa → 6500 K | subindo |
| Dia | 6500 K | teto pessoal, inicialmente 85% no Windows |
| Início da noite | 6500 → 3400 K | caindo |
| Pré-sono | 3400 → 2700 K | baixo |
| Noite | 1900–2300 K | mínimo confortável |

Gama do Windows 11 recusa rampas agressivas em silêncio. Estel não tenta escurecer a tela por gama abaixo de ~50 %: o extra vai para DDC (monitor externo) ou para a sobreposição (notebook / HDR).

No Windows 0.2.8, **Brilho mínimo**, **Máximo durante o dia** e **Máximo à noite / descanso**
são preferências separadas da intensidade da cor. O teto de descanso começa em
25% (nunca abaixo do piso escolhido), após o pôr do sol e das três horas antes
de dormir até acordar, inclusive em rotinas que cruzam a meia-noite ou dormem
de dia. Cor e brilho caminham gradualmente para os alvos; uma transição pode
levar alguns minutos. São pontos de partida ajustáveis, sem equivalência a lux
ou garantia de benefício clínico. O Android mantém sua implementação própria.

---

## O que foi deixado de fora

| Alegação | Evidência | Decisão |
|---|---|---|
| Óculos “bloqueadores de azul” | Podem não reduzir fadiga de curto prazo; baixa certeza para esse desfecho (Cochrane 2023) | Não |
| Batidas binaurais | Sem demonstração de benefício para este aplicativo | Não |
| Cor azul como tratamento calmante | Estudos de cor e emoção dependem do contexto e não testam o Estel | Sem promessa terapêutica por matiz |
| 432 Hz terapêutico | Fraco | Sem sino, sem alegação |
| Biometria / loop fechado | Fora do escopo + privacidade | Nunca |

Não tem conta nem coleta de imagens. As consultas online opcionais enviam a busca
digitada ou as coordenadas ao Open-Meteo; os quadros da câmera permanecem locais.

### Luz ambiente por sensor ou câmera (Windows)

O ajuste por luz ambiente vem desligado em novas instalações. Quando ativado,
pode preferir o sensor de iluminação do Windows; se ele não estiver disponível,
usa a câmera calibrada e depois horário/clima. O sensor fornece lux, enquanto
a câmera fornece apenas claridade relativa. Quem usar a webcam deve capturar **ambiente escuro** e,
depois, **ambiente claro**, mantendo câmera e tela na mesma posição e usando
luz ambiente difusa. As etapas podem ser feitas em momentos diferentes enquanto
o painel estiver aberto. Até as referências serem aceitas, a alternativa à câmera
é o sensor de luz habilitado ou horário/clima.
O Estel aguarda pelo menos 500 ms após abrir a câmera e confere cinco quadros,
calculando a média de até 8.000 pixels por quadro. Descarta os quadros em memória
e fecha o acesso. A leitura
padrão acontece a cada 30 segundos, tem limite de 5 segundos e o resultado é
suavizado antes de alterar o brilho. O Estel negocia resolução, formato e FPS
suportados, priorizando baixo tráfego de captura, e vincula as referências ao
perfil usado. Mudanças incompatíveis pedem nova calibração. Telas apagadas,
bloqueio de sessão e suspensão interrompem novas capturas e descartam leituras
em andamento; um helper já iniciado pode levar até cinco segundos para encerrar.

Se a câmera estiver indisponível, o brilho segue a curva por horário com uma
correção limitada pela radiação solar, se a consulta de clima estiver ativa.
Mudanças feitas na janela de configurações são aplicadas assim que são salvas.

Não há gravação, visualização ou transmissão dos quadros, identificação de pessoas, rosto, olhos,
presença ou estado emocional. Uma webcam comum não é um luxímetro: exposição
automática e posição da câmera mudam a leitura. Por isso o recurso trabalha
com a claridade da imagem como sinal experimental, sem calibração fotométrica.
As referências relativas substituem a conversão fixa de pixels; não calibram
a exposição nem transformam pixels em lux. Pares sem contraste suficiente,
quadros instáveis/saturados, outro dispositivo ou leituras fora das referências
são rejeitados. Se a exposição automática esconder a diferença entre ambientes,
use o ajuste por horário. Refaça as referências ao mover a câmera. Os sliders
de ambiente escuro/claro continuam ajustando a saída de brilho. Com a câmera ativa,
o brilho estimado corrige parcialmente a curva por
horário, respeitando os tetos pessoais de dia e descanso. O clima respeita os
mesmos tetos, inclusive antes de dormir quando ainda há sol. A câmera tem
prioridade sobre a estimativa de clima; desativar a opção restaura a curva com
clima opcional e não abre a câmera. Se usar o brilho automático nativo do Windows,
evite dois controladores automáticos simultâneos. Para testar só o brilho do Windows,
pause os ajustes de tela do Estel; isso também pausa sua alteração de cor.
Desligar apenas a câmera mantém o brilho por horário do Estel em funcionamento.

O processo principal aguarda eventos do Windows em vez de acordar a cada 50 ms;
esse intervalo curto fica reservado às transições de áudio. Chamadas de drivers
ficam em um processo separado com prazo e uma única intenção pendente. O painel
atualiza informações por evento, sem consultar o arquivo de clima a cada quadro.
O protocolo de medição e seus limites estão em [desempenho no Windows](docs/WINDOWS-PERFORMANCE.md).

### Localização, clima e orientação (Windows)

Na primeira instalação, o guia oferece a localização do Windows como opção;
a solicitação só começa quando você a ativa. Se preferir ou se o acesso for
negado, busque cidade ou bairro pelo botão **Buscar**,
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
aproximada do sol e a radiação direta para estimar um acréscimo limitado de
brilho. Não mede nem remove reflexos, cortinas ou luz artificial no cômodo.
A câmera, quando funciona, sempre tem prioridade sobre essa estimativa.
Se conectar ou reconectar a webcam com o painel aberto, use **Buscar câmeras
novamente** na seção de luz ambiente. A câmera calibrada é localizada pela
identidade do dispositivo mesmo quando o Windows altera a ordem das webcams.
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

Pesquisa atualizada em **29/09/2026**. A
[auditoria de estudos recentes e do algoritmo](docs/RESEARCH-AUDIT-2026.md)
compara a qualidade das fontes, os ajustes com e sem webcam e as mudanças
justificadas. O [catálogo de evidências](docs/COMFORT-EVIDENCE.md) detalha cada recurso.
Publicação recente não equivale a evidência de alta certeza: nenhuma das fontes
abaixo valida os coeficientes do Estel ou uma webcam sem calibração como luxímetro.

### Base atual e atualizações de 2024–2026

- [CIE PS 001:2024](https://www.cie.co.at/publications/cie-position-statement-integrative-lighting-recommending-proper-light-proper-time-3rd)
  — posição atualizada sobre luz no horário adequado e metrologia CIE S 026.
- [ISO/CIE 8995-1:2025](https://committee.iso.org/standard/76342.html?browse=tc)
  — iluminação de locais de trabalho; requisitos do ambiente, não porcentagem ideal de tela.
- [Spitschan et al., BMJ Public Health, 2025](https://doi.org/10.1136/bmjph-2025-003205)
  — consenso Delphi de comunicação sobre luz e saúde; não ensaio de um aplicativo.
- [Spitschan et al., BMC Medicine, 2026](https://pubmed.ncbi.nlm.nih.gov/41612386/)
  — consenso sobre lacunas de medição, dose–resposta e eficácia de intervenções.
- [Yang et al., 2026](https://doi.org/10.2150/jstl.IEIJ250000672)
  — revisão de parâmetros de luz e fadiga digital; contexto mecanístico, sem curva validada para webcam.
- [Massa et al., 2025](https://pubmed.ncbi.nlm.nih.gov/42376338/)
  — ensaio de brilho e software de cor, com 47 participantes que concluíram; evidência preliminar.
- [Redondo et al., 2025](https://doi.org/10.1016/j.exer.2025.110463)
  — pausas durante leitura; não estabelece uma regra universal de 20 segundos.
- [TFOS DEWS III, 2025](https://doi.org/10.1016/j.ajo.2025.05.039)
  — revisão de manejo do olho seco; não valida regulagem de brilho por câmera.
- [Xu et al., npj Digital Medicine, 2025](https://doi.org/10.1038/s41746-025-02053-8)
  — ensaio pequeno de treino de piscar em usuários de smartphone com olho seco.
- [CIE 252:2024](https://www.cie.co.at/publications/assessment-discomfort-glare-daylight-buildings)
  — avaliação de ofuscamento por luz diurna; azimute e clima sozinhos não medem reflexos.
- [CIE 249:2022 e corrigenda de 2026](https://www.cie.co.at/publications/visual-aspects-time-modulated-lighting-systems)
  — modulação temporal da luz; exige avaliação do hardware.
- [Basner et al., SLEEP, 2026](https://doi.org/10.1093/sleep/zsag001)
  — estudo controlado de ruído rosa durante sono; não justifica som automático para dormir.
- [Brown TM et al., PLOS Biology, 2022](https://doi.org/10.1371/journal.pbio.3001571)
  — consenso melanópico EDI, mantido e complementado pela CIE de 2024.
- [Singh S et al., Cochrane, 2023, CD013244](https://www.cochrane.org/evidence/CD013244_blue-light-filtering-spectacle-lenses-visual-performance-macular-back-part-eye-protection-and)
  — 17 ensaios de óculos, não de filtros de tela; certeza varia por desfecho.
- [OSHA — iluminação em estações de computador](https://www.osha.gov/etools/computer-workstations/workstation-environment)
  — reflexos, contraste e fadiga visual.

### Estudos de contexto e referências históricas

- Wilkins AJ et al. *Lighting Research & Technology* 21(1):11–18, 1989 — flicker e cefaleia
- [Hazell & Wilkins, 1990](https://doi.org/10.1017/S0033291700017098)
  — lâmpadas fluorescentes e agorafobia; não teste de escurecimento de tela.
- [Wilms L & Oberfeld D, 2018](https://doi.org/10.1007/s00426-017-0880-8)
  — brilho, saturação e emoção em estímulos controlados, não tratamento de ansiedade.
- [Weijs et al., 2023](https://pubmed.ncbi.nlm.nih.gov/37830019/)
  — cor e excitação em RV; corrige a autoria principal antes indicada como Reutimann.
- [IEEE 1789-2015](https://standards.ieee.org/ieee/1789/4479/)
  — referência histórica de modulação de LEDs; **Inactive-Reserved desde 26/03/2026**.
- [Blumenthal TD & Berg WK, 1986](https://doi.org/10.1111/j.1469-8986.1986.tb00682.x)
  — estímulos acústicos breves e intensos; não valida o fade de quatro segundos do Estel.
- Sheedy JE et al. *Ergonomics* 48(9):1114–1128, 2005,
  [doi:10.1080/00140130500208414](https://doi.org/10.1080/00140130500208414)
  — luminância ao redor da tela e adaptação visual
- [ISO/TR 9241-610:2022](https://www.iso.org/obp/ui/en/#iso:std:iso:tr:9241:-610:ed-1:v1:en)
  — impacto da luz e da iluminação em sistemas interativos
