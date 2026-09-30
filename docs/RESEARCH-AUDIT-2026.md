# Auditoria científica da autorregulagem — 29/09/2026

## Conclusão

Há fontes mais recentes e mais pertinentes para completar a base. Elas sustentam
ajustar luz ao ambiente, preservar legibilidade e considerar o horário de sono.
Não validam a curva numérica do Estel, a média de pixels como medida ambiental
ou a combinação automática de webcam, clima, localização, cor e som.

A principal fragilidade é **metrológica**: o algoritmo recebe claridade de uma
imagem processada, não uma medida calibrada da luz no ambiente. Melhorar esse
sinal e incorporar a preferência da pessoa tem prioridade sobre trocar números
com base na data de um artigo. A seção final registra as correções de engenharia
implementadas no Windows 0.2.8 e as medições que ainda não foram realizadas.

## Método e grau de confiança

Pesquisa dirigida em páginas de periódicos, PubMed/PMC, Cochrane, CIE, ISO e
documentação dos fabricantes, com foco em 2024–29/09/2026 e conferência das
referências anteriores. Termos incluíram `digital eye strain interventions
systematic review`, `screen luminance ambient illuminance`, `melanopic light
consensus`, `camera photometric calibration auto exposure` e `pink noise sleep`.
É uma atualização crítica direcionada, não revisão sistemática registrada,
busca exaustiva ou avaliação formal GRADE feita pelo projeto.

A prioridade é adequação à pergunta, método, medidas, controle de viés e
incerteza. Nome de instituição, indexação e ano não garantem conclusão correta.
Consenso técnico ajuda a definir medidas; não equivale a ensaio de eficácia.
Metanálise de ensaios pode ter baixa certeza quando os ensaios são frágeis.
Casos Apple/Google/Microsoft informam engenharia, não comprovam redução de ansiedade.

As páginas públicas e os resumos foram conferidos. Quando o texto integral não
estava acessível, a decisão ficou limitada ao resumo; não atribuímos à fonte
um método ou certeza ausente nele. Os textos integrais comerciais das normas
CIE/ISO não foram adquiridos: os catálogos oficiais confirmam escopo e edição,
sem autorizar alegação de conformidade ou copiar tabelas de requisitos.

## Fontes prioritárias e novidades

| Fonte verificada | Método e limite | Consequência para o Estel |
|---|---|---|
| [CIE PS 001:2024](https://www.cie.co.at/publications/cie-position-statement-integrative-lighting-recommending-proper-light-proper-time-3rd), terceira edição | Posição da comissão de iluminação; reafirma CIE S 026 e luz adequada de dia, menor exposição nas três horas antes de dormir. Complementa Brown 2022. | Horário de dormir importa, não apenas pôr do sol. Kelvin e porcentagem de tela não certificam exposição melanópica. |
| [ISO/CIE 8995-1:2025](https://committee.iso.org/standard/76342.html?browse=tc) | Norma atual de iluminação interna de trabalho; conforto, desempenho e qualidade/quantidade de luz. Substitui ISO 8995-1:2002, não ISO/TR 9241-610:2022. Escopo público consultado. | Considerar o ambiente inteiro e a tarefa; não converter um requisito de iluminação em porcentagem universal do monitor. |
| [Spitschan et al., BMJ Public Health, 2025](https://doi.org/10.1136/bmjph-2025-003205) | Delphi modificado, 21 especialistas, 26 mensagens com consenso. Comunicação científica, não ensaio clínico. | Reforça regularidade e importância da exposição ocular; não estabelece dose para webcam ou tratamento pelo Estel. |
| [Spitschan et al., BMC Medicine, 29/01/2026](https://pubmed.ncbi.nlm.nih.gov/41612386/), DOI 10.1186/s12916-025-04608-8 | Consenso de 13 especialistas identifica nove lacunas, incluindo medição padronizada, estimativas de exposição e eficácia de intervenções. O “2025” no DOI não é o ano de publicação. | A publicação moderna explicita incertezas, em vez de fornecer uma fórmula universal nova. É incorreto declarar a autorregulagem clinicamente validada. |
| [CIE 244:2021 — comunicado oficial](https://cms.cnc-cie.ca/images/documents/PressRelease/CIE244_2021.pdf) | Caracterização e calibração de dispositivos que medem luminância por imagem. Fonte técnica específica, mesmo anterior a 2024. | Webcam só se torna instrumento de medida após caracterização e calibração adequadas; sliders de saída não fazem isso. |
| [CIE 252:2024](https://www.cie.co.at/publications/assessment-discomfort-glare-daylight-buildings) | Ofuscamento diurno depende de luminância, fundo, posição e tamanho aparente da fonte. Escopo público consultado. | Clima e direção da janela não bastam para medir ofuscamento; aumentar brilho não remove reflexo. |
| [Yang et al., 24/04/2026](https://doi.org/10.2150/jstl.IEIJ250000672) | Revisão de parâmetros luminosos, 89 referências. Resumo e metadados consultados; não foi usada como metanálise com certeza alta. | Tanto excesso de brilho quanto iluminação insuficiente podem ser problemáticos; não buscar “o mais escuro possível”. Não fornece calibração ou coeficiente para o Estel. |
| [Massa et al., 28/09/2025](https://pubmed.ncbi.nlm.nih.gov/42376338/) | Ensaio de um mês: 74 inscritos, 47 concluintes; 17 reduziram brilho, 19 usaram f.lux, 11 controle. Sintomas autorrelatados; melhora intragrupo não prova diferença entre grupos. | Resultado preliminar favorece estudar brilho confortável, não declarar eficácia ou importar um número. Já constava no catálogo; faltava ao README. |
| [Redondo et al., 2025](https://doi.org/10.1016/j.exer.2025.110463) | Experimento de leitura por 40 minutos, comparando esquemas de pausa; resumo consultado. Pausas a cada dez minutos ou por escolha tiveram resultados favoráveis em alguns desfechos. | Apoia oferecer pausas flexíveis; não torna obrigatório um esquema nem invalida toda pausa de 20 segundos. Já constava no catálogo. |
| [TFOS DEWS III: Management and Therapy, 2025](https://doi.org/10.1016/j.ajo.2025.05.039) | Revisão clínica de manejo do olho seco, publicada antecipadamente em junho e em edição de novembro. Resumo/metadados consultados. O projeto TFOS recebeu doações de empresas do setor, declaradas na publicação. | Referência pertinente para não reduzir todo desconforto a brilho/cor; não estabelece regulador de tela nem dispensa avaliar a qualidade de cada intervenção. |
| [Xu et al., npj Digital Medicine, 20/11/2025](https://doi.org/10.1038/s41746-025-02053-8) | Ensaio de treino de piscar: 40 estudantes com olho seco, 37 mulheres, 30 dias. Melhoras em sintomas e medidas oculares; população restrita e amostra pequena. | Justifica estudar lembretes voluntários de piscar. Não copiar automaticamente barras visuais a cada cinco segundos para um app de foco/ansiedade, nem generalizar o efeito para todos. |
| [Basner et al., SLEEP, 2026](https://doi.org/10.1093/sleep/zsag001) | Estudo controlado em laboratório, 25 adultos, sete noites, medidas de sono. Ruído rosa contínuo prejudicou aspectos da arquitetura do sono nas condições testadas. | Manter áudio por escolha e sem promessa de sono melhor. Não extrapolar para todo volume, ruído marrom ou uso enquanto acordado. |
| [Vincens et al., Communications Medicine, 2026](https://doi.org/10.1038/s43856-026-01380-5) | Piloto cruzado, 12 adultos, ruído de tráfego e mascaramento rosa. Condições distintas do estudo acima. | Resultados de mascaramento dependem do contexto. Um piloto favorável não anula o estudo contrário nem justifica ativar ruído para todos. |
| [Wang et al., General Hospital Psychiatry, 2026](https://doi.org/10.1016/j.genhosppsych.2026.04.014) e [Menegaz de Almeida et al., JAMA Psychiatry, 2024](https://jamanetwork.com/journals/jamapsychiatry/fullarticle/2824482) | Metanálises de fototerapia em populações clínicas; a de 2024 reúne 11 ensaios/858 pacientes. Equipamento, dose, horário e população importam. Resumos consultados. | Existem benefícios estudados da luz para saúde mental. Eles não demonstram que escurecer/aquecer uma tela trate ansiedade, pânico ou depressão. Não importar uma dose clínica para o monitor. |

### O que manter das referências anteriores

- [Brown et al., 2022](https://doi.org/10.1371/journal.pbio.3001571): continua
  pertinente e é incorporado à posição CIE de 2024. As recomendações se referem
  a adultos saudáveis com rotina diurna e luz medida no plano dos olhos.
- [Cochrane 2023](https://www.cochrane.org/evidence/CD013244_blue-light-filtering-spectacle-lenses-visual-performance-macular-back-part-eye-protection-and):
  17 ensaios/619 participantes; baixa certeza para fadiga de curto prazo e
  resultados incertos para sono. Testar óculos não equivale a testar um app.
  A frase anterior “provavelmente nulo” era ampla demais e foi corrigida.
- [Singh et al., Ophthalmology, 2022](https://doi.org/10.1016/j.ophtha.2022.05.009):
  revisão sistemática com 45 ensaios/4.497 participantes e avaliação GRADE.
  Não encontrou evidência de alta certeza para as terapias analisadas. Seu método
  é mais informativo que uma revisão narrativa recente, mas não ensaia nossa curva.
- [TFOS Lifestyle: ambiente digital, 2023](https://doi.org/10.1016/j.jtos.2023.04.004):
  relatório específico de fadiga digital, com parte narrativa e revisão
  sistemática registrada de ensaios sobre superfície ocular. A seção de
  ergonomia não deve herdar automaticamente a certeza da parte sistemática.
  Complementa pausas/piscar, tamanho de texto e ambiente; não calibra nossa câmera.
- [Sheedy et al., 2005](https://pubmed.ncbi.nlm.nih.gov/16251151/): 37 participantes,
  níveis de luminância controlados, diferenças de adaptação e ampla preferência
  individual. Informação ergonômica específica; não curva clínica para webcam.
- [Hazell e Wilkins, 1990](https://doi.org/10.1017/S0033291700017098),
  [Wilms e Oberfeld, 2018](https://doi.org/10.1007/s00426-017-0880-8),
  [Weijs et al., 2023](https://pubmed.ncbi.nlm.nih.gov/37830019/) e
  [Blumenthal e Berg, 1986](https://doi.org/10.1111/j.1469-8986.1986.tb00682.x):
  contexto sobre lâmpadas, cor/RV e sobressalto, respectivamente. Não comprovam
  paleta terapêutica, regulagem de monitor contra pânico ou fade ideal de quatro segundos.
- [IEEE 1789-2015](https://standards.ieee.org/ieee/1789/4479/): o catálogo registra
  **Inactive-Reserved em 26/03/2026**, por processo administrativo. Isso não
  demonstra que suas conclusões eram falsas. A referência atual é complementada
  por [CIE 249:2022 e corrigenda de 2026](https://www.cie.co.at/publications/visual-aspects-time-modulated-lighting-systems).
  Nenhum desses documentos permite afirmar que o Estel elimina PWM.

## Comparação com o algoritmo da versão 0.2.7

Base auditada: commit `b3c3f5b3865d42ee9c5782175c4286389393f463`.
Os apontadores abaixo são desta revisão; linhas podem mudar em versões futuras.

| Caminho | O que o código faz | Avaliação |
|---|---|---|
| Webcam | Abre a câmera, usa o primeiro quadro disponível, calcula média YUY2; mapeia `Y^0,55` para uma faixa de brilho e suaviza 20% (`src/ambient.rs:169`, `src/ambient.rs:336`, `src/ambient.rs:351`). | A imagem é processada pelo dispositivo. A leitura não normaliza exposição/ganho nem calibra luminância. Superfícies, enquadramento e luz da própria tela podem mudar a média. Até a interpretação como sinal relativo precisa de verificação no hardware. |
| Webcam + horário | Corrige 35% da diferença e, entre pôr e nascer do sol, não aumenta o alvo acima da curva (`src/main.rs:294`, `src/main.rs:683`, `src/main.rs:692`). | Peso e limite são escolhas de engenharia. Suavização reduz oscilação, não corrige erro sistemático de exposição. Noite solar não equivale ao período antes de dormir. |
| Sem webcam | Mantém curva por sol/rotina; clima opcional acrescenta até 25 pontos a partir de radiação e janela. Sem sol acima do horizonte, não acrescenta (`src/weather.rs:205`). | É previsão externa, não sensor da mesa. Cortinas, edifícios, posição real e luz artificial ficam sem medida. Não detectar um reflexo não é removê-lo. |
| Intensidade e mínimo | “Média” aplica fator 0,6 também ao brilho; `1 + (brilho − 1) × fator`. Mínimo padrão 25% (`src/config.rs:29`, `src/config.rs:141`, `src/target.rs:48`). | O mínimo é piso, não alvo noturno universal. Um ponto de 16% vira 49,6% em Média antes das outras etapas. Os 25% confortáveis em uma configuração pessoal não significam que todos recebem esse brilho ou que todos devem recebê-lo. |
| Cor/saída | Curva de Kelvin e adaptação em mired; limite temporal, DDC/gama/camada (`src/main.rs:277`, `src/main.rs:306`, `src/target.rs:48`). | Controle técnico útil, sem medida da exposição espectral nos olhos. Não traduzir Kelvin ou porcentagem em dose biológica. |
| Som | Desligado em configuração nova (`src/config.rs:146`). | Mantém a decisão coerente com incerteza e diferenças individuais; nenhum estudo recente seleciona automaticamente rosa/marrom conforme hora para todo usuário. |

O risco de exposição automática é confirmado pela
[documentação Microsoft de câmera](https://learn.microsoft.com/en-us/windows-hardware/drivers/stream/camera-settings-page).
A [calibração radiométrica SPECTACLE, Optics Express, 2019](https://doi.org/10.1364/OE.27.019075)
mostra diferenças entre câmeras e processamento; sustenta caracterizar o sinal,
não presumir equivalência a sensores ambientais. Não usar rosto/emoção é compatível
com manter a análise apenas de luz, mas não resolve essa calibração.

## Casos de produto: o que realmente podemos aproveitar

- [Apple](https://support.apple.com/en-au/109351): brilho automático usa sensor
  de luz ambiente; agenda Night Shift é outro recurso. Localização determina
  horários, não iluminância no cômodo.
- [Windows](https://learn.microsoft.com/en-us/windows-hardware/design/device-experiences/sensors-adaptive-brightness):
  calibração do sensor e painel, curvas com faixas sobrepostas para evitar
  oscilação, transições suaves e testes contra instrumentos de referência.
  A tabela em lux não pode ser aplicada aos pixels da webcam.
- [Android/Google](https://android-developers.googleblog.com/2018/11/getting-screen-brightness-right-for.html):
  ajuste adaptativo incorpora mudanças manuais e preferência por condições de
  luz. Caso documentado de 2018, útil pelo mecanismo; não alegação de inovação de 2026.

Para experimentar o brilho do Windows sozinho, é necessário pausar os ajustes
de tela do Estel, inclusive sua cor. Desativar só a câmera mantém a curva por
horário funcionando (`src/main.rs:274`, `src/main.rs:306`). A preferência por um
sensor real não significa que o aplicativo já tenha integração exclusiva com ele.

## Mudanças justificadas e ordem de prioridade

1. **Tratar a webcam como experimental até validar seu sinal.** Para evoluir:
   caracterizar exposição/ganho e resposta da câmera, estabilidade após abertura,
   saturação e influência da tela; comparar com referência física. Quando não
   houver sinal confiável, usar horário/preferência em vez de fingir lux. Travar
   exposição indiscriminadamente pode prejudicar a imagem e não substitui calibração.
2. **Guardar o brilho confortável como preferência explícita.** Separar esse
   controle da intensidade do aquecimento; o comportamento atual aproxima ambos
   do neutro. Testar cada monitor, com ambientes claros/escuros e conteúdo distinto.
   A preferência é calibração de conforto, não calibração fotométrica.
3. **Considerar as três horas antes de dormir além do pôr do sol.** A proteção
   atual é solar; pode deixar webcam/clima elevar brilho quando a pessoa dorme
   cedo e ainda é dia. Um limite pela rotina, com possibilidade de ajuste, é
   uma inferência de produto apoiada pela CIE, não garantia de atingir 10 lux
   melanópicos. Exige tratar cruzamento da meia-noite e rotinas fora do horário diurno.
4. **Manter clima/janela como aproximação opcional e não cumulativa.** Uma futura
   estimativa de reflexos precisa de informação espacial/fotométrica, não apenas
   radiação externa. Não há justificativa para ativar todas as fontes por padrão.
5. **Preservar pausas flexíveis, som opcional e apoio por escolha.** Priorizar
   legibilidade, estabilidade e controle pessoal durante o uso. Evidência de
   fototerapia ou som durante sono não fornece automaticamente uma intervenção
   contra ansiedade para a tela.

## Correções implementadas no Windows 0.2.8

- `src/target.rs`: intensidade de cor/som preserva o brilho. Um alvo de 16%
  permanece 16% antes do piso, em Alta, Média e Suave.
- `src/config.rs` e `src/comfort.rs`: piso e tetos pessoais separados. Padrão
  inicial de dia 85%, descanso 25%, respeitando o piso existente. Descanso
  considera noite solar e as três horas antes de dormir até acordar; cobre
  meia-noite e sono diurno. A cor também segue uma rampa pela rotina.
- `src/main.rs`: câmera e clima respeitam os mesmos limites e não somam suas
  correções. O alvo de descanso é gradual; a transição pode levar minutos.
  Uma leitura rejeitada sai imediatamente do cálculo; o limitador evita salto.
- `src/ambient.rs`, `src/config.rs` e `src/ui.rs`: a conversão fixa `Y^0,55`
  foi substituída por interpolação entre referências relativas escura/clara
  da mesma câmera. Capturas têm prazo de cinco segundos e são serializadas.
  Após 500 ms, cinco quadros precisam ser estáveis, não saturados; referências
  precisam diferir em pelo menos 0,10 e leituras fora da faixa são rejeitadas.
  Diferenças inferiores a dois pontos na saída suavizada não movem o alvo.
  Sem referências válidas, não usa a webcam como entrada de brilho.
- Som permanece opcional e desligado por padrão; pausas e apoio permanecem
  ações voluntárias. A atualização não converte estudos de fototerapia em
  promessas terapêuticas para a tela nem altera o Android.

Esses coeficientes são filtros de engenharia, **não resultados clínicos novos**.
Foram removidas suposições frágeis e acrescentados limites pessoais; as referências
relativas não corrigem toda a exposição automática, ganho, enquadramento ou luz
da tela. Ainda são necessários instrumentos de referência e medições em vários
dispositivos/cômodos para estabelecer erro fotométrico. Avaliar conforto e foco
exige comparação adequada; satisfação individual não demonstra eficácia clínica.
