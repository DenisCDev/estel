# Adaptação ao hardware no Windows

O Estel usa as capacidades informadas pelo Windows e pelo dispositivo. Não é possível descobrir com confiança todas as propriedades físicas de uma tela por software: painel IPS/VA, mini-LED, PWM e conforto percebido não são inferidos do nome comercial. A implementação usa `windows@0.62.2`, `eframe@0.32` e a toolchain fixada no repositório; o arquivo `Cargo.lock` define as versões resolvidas.

| Situação identificada | Comportamento | Limite |
| --- | --- | --- |
| Saída SDR identificada, sem clonagem | Captura a gama original, aplica aquecimento e confirma por leitura | O driver pode recusar; preserva o original e informa a pendência |
| HDR, cor ampla/ACM ou modo desconhecido | Não usa `SetDeviceGammaRamp`; mantém a configuração de cor do Windows | Aquecimento por sobreposição é aproximado e pode elevar sombras |
| Monitor externo com brilho DDC/CI | Ajusta o brilho físico naquela saída | Firmware, docks e cabos podem não suportar ou responder |
| Painel interno com WMI compatível | Usa brilho nativo e níveis anunciados pelo painel | Não presume WMI em qualquer notebook |
| Monitor sem controle físico | Escurece com camada preta individual | Isso não comprova economia elétrica, especialmente em LCD |
| Telas com capacidades diferentes | Decide por saída e refaz a identificação após mudanças | Cores e luminância percebidas ainda variam entre monitores |
| Monitor ausente durante recuperação | Conserva seu registro e permite ajustes seguros nas telas presentes | Não declara restauração global completa |
| Sensor de iluminação do Windows | Quando a luz ambiente está ativada, pode preferir leitura em lux | A curva de brilho é preferência de conforto, não calibração fotométrica |
| Webcam | Negocia YUY2/NV12 econômicos; MJPG com conversão como alternativa | Exposição automática continua experimental, sem equivalência a lux |
| Sessão bloqueada, suspensa ou tela apagada | Pausa novas capturas e ajustes visuais; descarta leitura obsoleta | Uma captura já iniciada pode levar até cinco segundos para retornar ao host |

Os controladores físicos e a topologia ficam em `src/display_topology.rs`, `src/display.rs`, `src/brightness.rs` e `src/hardware_wmi.rs`. O isolamento em `src/hardware_worker.rs` tem prazo de 12 segundos por pedido e apenas uma intenção pendente. O mesmo limite cobre a descoberta inicial e a redescoberta de uma mudança detectada durante qualquer pedido; a interface continua respondendo enquanto aguarda. O encerramento reserva até 25 segundos para terminar um pedido em curso e restaurar as telas. A pausa só confirma restauração após resposta; falhas enquanto desativado recebem até três tentativas, separadas por cinco e dez segundos.

Na execução local, a identificação das duas telas SDR concluiu em 4,78 s, enquanto o prazo anterior de quatro segundos interrompia o mesmo diagnóstico. O limite foi ampliado sem aumentar a frequência das consultas. A [medição de descoberta](../artifacts/performance/display_discovery_030.json) registra a falha e a conclusão do auxiliar isolado; esse diagnóstico não alterou o brilho nem a gama.

Os registros em `src/session.rs` são gravados antes da primeira alteração e vinculados à identidade da saída. A restauração de gama também exige leitura de confirmação. Uma versão antiga que nunca salvou a gama original não permite reconstruí-la: o Estel informa essa limitação e não grava uma rampa identidade como se fosse a calibração anterior.

O loop principal usa eventos de janela, sessão, energia e resultados dos auxiliares (`src/runtime.rs`). O intervalo de 50 ms fica reservado às transições de áudio. O painel mantém estados em memória e recebe avisos de mudança (`src/status.rs`), incluindo encerramento do processo principal. A câmera amostra até 8.000 pixels por quadro, acessa o buffer bidimensional sem copiá-lo inteiro e associa a calibração ao perfil de captura (`src/ambient.rs`). Nenhum quadro é salvo ou transmitido.

Essas decisões buscam previsibilidade, controle pessoal e baixo custo de execução. Elas não demonstram que o aplicativo trate ansiedade ou melhore foco para todas as pessoas. A base científica e seus limites permanecem em [conforto visual](COMFORT-EVIDENCE.md); os resultados efetivamente medidos estão em [desempenho no Windows](WINDOWS-PERFORMANCE.md).

## Fontes de plataforma

- [SetDeviceGammaRamp](https://learn.microsoft.com/en-us/windows/win32/api/wingdi/nf-wingdi-setdevicegammaramp): retorno de sucesso não prova aplicação; comportamento em HDR é indefinido.
- [QueryDisplayConfig](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-querydisplayconfig) e [informações modernas de cor](https://learn.microsoft.com/en-us/windows/win32/api/wingdi/ns-wingdi-displayconfig_get_advanced_color_info_2): topologia e distinção de pipelines; versões antigas recebem alternativa conservadora.
- [GetMonitorCapabilities](https://learn.microsoft.com/en-us/windows/win32/api/highlevelmonitorconfigurationapi/nf-highlevelmonitorconfigurationapi-getmonitorcapabilities), [GetMonitorTechnologyType](https://learn.microsoft.com/en-us/windows/win32/api/highlevelmonitorconfigurationapi/nf-highlevelmonitorconfigurationapi-getmonitortechnologytype) e [WmiSetBrightness](https://learn.microsoft.com/en-us/windows/win32/wmicoreprov/wmisetbrightness-method-in-class-wmimonitorbrightnessmethods): capacidades reais e brilho físico.
- [LightSensor](https://learn.microsoft.com/en-us/uwp/api/windows.devices.sensors.lightsensor?view=winrt-26100), [formatos nativos de captura](https://learn.microsoft.com/en-us/windows/win32/api/mfreadwrite/nf-mfreadwrite-imfsourcereader-getnativemediatype) e [buffers de vídeo](https://learn.microsoft.com/en-us/windows/win32/medfound/uncompressed-video-buffers): negociação e acesso aos dados locais.
- [Identificadores de energia](https://learn.microsoft.com/en-us/windows/win32/power/power-setting-guids) e [notificações de sessão](https://learn.microsoft.com/en-us/windows/win32/api/wtsapi32/nf-wtsapi32-wtsregistersessionnotification): suspensão de trabalho quando a sessão deixa de estar disponível.

Testes simulados de HDR, recuperação e formatos não substituem ensaios físicos. O computador usado nesta medição tem duas telas SDR externas e uma Logitech C270; a validação física em HDR/OLED/notebook depende desses equipamentos. O Windows pode informar capacidades incompletas: nesses casos o painel mostra a incerteza.
