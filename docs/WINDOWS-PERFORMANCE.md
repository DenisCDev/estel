# Medição de recursos no Windows

## Referência instalada em 02/10/2026

As medições abaixo são do executável instalado **0.2.9**, antes das alterações em desenvolvimento. Seu hash e as opções relevantes estão no [contexto sanitizado](../artifacts/performance/baseline_029_contexto.json). Clima e áudio estavam desligados; tela estava habilitada; câmera estava habilitada, aguardando referências, sem captura periódica ativa. A configuração foi preservada. As janelas foram abertas e fechadas pela interface; o processo principal não foi encerrado à força.

Cada cenário solicitou 60 s com amostras a cada 2 s. A coleta parou de iniciar consultas perto do fim do prazo e produziu aproximadamente 58,3 s e 29 amostras por processo. CPU é porcentagem da capacidade total dos 12 processadores lógicos; memória está em MiB (1.048.576 bytes). CPU dos processos usa a diferença de tempo acumulado entre primeira e última amostra. As médias de memória são aritméticas. [Resumo com resultados por cenário](../artifacts/performance/baseline_029_resumo.json).

| Cenário e processo | CPU média | Memória residente média | Memória privada média |
| --- | ---: | ---: | ---: |
| Bandeja, principal | 0,01167% | 28,338 MiB | 3,662 MiB |
| Painel ocioso, principal | 0,01399% | 28,397 MiB | 3,700 MiB |
| Painel ocioso, janela de configurações | sem incremento detectado | 111,753 MiB | 138,432 MiB |
| Bandeja, DWM do desktop inteiro | 6,24138% | 77,481 MiB | 214,154 MiB |
| Painel ocioso, DWM do desktop inteiro | 7,24425% | 77,262 MiB | 214,438 MiB |

Dados brutos: [bandeja](../artifacts/performance/baseline_029_bandeja_20261002T135146906Z_33320.json), [painel e DWM](../artifacts/performance/baseline_029_painel_dwm_20261002T135509450Z_33320.json). A primeira [coleta do painel](../artifacts/performance/baseline_029_painel_aberto_20261002T134750463Z_33320.json), com atividade de interface e uma falha de leitura direta do DWM, ficou separada: a janela registrou 0,22745% de CPU. As duas amostras do painel não são intercambiáveis para comparar versões.

O DWM compõe o desktop inteiro, incluindo outros aplicativos. Sua diferença entre cenários não pode ser atribuída ao Estel. Seus contadores formatados têm intervalo e precisão próprios. Os motores 3D do DWM indicaram médias de aproximadamente 8,90% e 8,34%; isso não representa a carga total da GPU nem comprova o custo isolado dos overlays. Ausência de motor para um processo é dado indisponível, não uma leitura de zero; zero no contador é a precisão observada, não ausência absoluta de trabalho. [Como o Windows mede motores de GPU](https://devblogs.microsoft.com/directx/gpus-in-the-task-manager/).

O próprio coletor consumiu 5,3125 s de CPU na coleta da bandeja e 7 s na coleta do painel com DWM. Esses custos estão separados do aplicativo. As consultas podem influenciar o sistema medido; as amostras não constituem um ensaio isolado ou estatisticamente suficiente para prometer redução percentual. Memória residente compartilhada pode ser contada duas vezes se somada entre processos. Filhos muito curtos podem passar entre amostras.

## Leitura pontual da câmera

O auxiliar instalado `--sample-ambient 0`, executado com autorização, concluiu em 1,2328 s, com 0,125 s de CPU observada e pico de memória residente de 29,465 MiB. Retornou uma leitura válida e saiu com código 0. A amostragem a cada 100 ms pode perder o trecho final de CPU. Nenhuma imagem ou identificação de câmera foi gravada. É uma leitura pontual sem calibração nem alteração das telas; não mede lux. [Resultado](../artifacts/performance/baseline_029-camera.json).

## Reprodução

Em PowerShell 7, obtenha o PID do processo principal do Estel instalado e execute:

```powershell
./scripts/Measure-Estel.ps1 -TargetProcessId 1234 -Scenario bandeja_nova_versao `
    -DurationSeconds 60 -IntervalSeconds 2 -IncludeGpu -IncludeDwm
```

O script não inicia, fecha ou altera o aplicativo. Troque `1234` pelo PID real. Gere a referência e a versão nova com a mesma configuração, conteúdo na tela, número de monitores, energia e aplicativos em segundo plano, sem compilar ou baixar arquivos durante a coleta. Repita os cenários e compare processos equivalentes, incluindo a janela filha. Os JSON/CSV ficam em `artifacts/performance`; falhas de consultas ficam nos avisos, sem serem convertidas em consumo zero. Consultas CIM têm prazo de 2 s e três falhas consecutivas suspendem as consultas de GPU; o provedor do Windows pode atrasar o cancelamento.

Para medir o auxiliar de câmera, com permissão de uso da câmera e apontando para um executável Estel conhecido:

```powershell
./scripts/Measure-EstelCamera.ps1 -Executable 'CAMINHO_DO_ESTEL.exe' `
    -CameraIndex 0 -Scenario camera_nova_versao
```

O prazo de 5 s encerra somente esse auxiliar isolado. Não use esse método para encerrar o processo principal: a saída normal precisa restaurar o estado de tela.

## Energia e limites ainda abertos

Watts, joules e energia elétrica do computador ou dos monitores **não foram medidos**. WPR ofereceu os perfis Power e GPU, mas a tentativa de gravação falhou com `0xc5585011`, sem alterar políticas ou elevar privilégios. A consulta posterior confirmou que nenhuma gravação ficou ativa. [Erro do WPR](../artifacts/performance/wpr-start-errors.txt), [estado posterior](../artifacts/performance/wpr-status-after.txt). A Microsoft documenta esse erro quando a coleta não dispõe de privilégios administrativos. Os perfis e as métricas dependem dos recursos de rastreamento do sistema; CPU/RAM/GPU são indicadores de recursos, não substitutos de um medidor elétrico. [Procedimento de análise de CPU e GPU com WPR/WPA](https://learn.microsoft.com/en-us/windows/apps/performance/power), [métricas disponíveis no WPA](https://learn.microsoft.com/en-us/windows-hardware/test/wpt/list-of-wpa-graphs).

O bloqueio anterior de uma DLL procedural em Code Integrity foi registrado no [diagnóstico sanitizado](../artifacts/performance/rust_196_code_integrity.json). Após a desativação manual do Smart App Control neste computador, a compilação local e a execução do pacote da CI foram aceitas. Isso não confere assinatura ou identidade de editor ao executável. Um menor uso de CPU, ou escurecimento por overlay, não prova por si só menor consumo elétrico da tela.

## Execução da versão 0.3.1 em 02/10/2026

O executável release da [CI 37040813057](https://github.com/DenisCDev/estel/actions/runs/37040813057), do commit `2b6fad53f6facd3eed78a1eb89ccd01a4a6171c5`, foi executado com a configuração existente. Seu SHA-256 é `C0C49CDEA9F5D06157EEAE7D3F2F8E043F3B4707B7E1F8853E35D4E0F5F6FB96`. A migração apenas adicionou os campos `setup_completed=true` e `ambient_prefer_light_sensor=true`, mantendo todos os valores anteriores. Os dois monitores externos SDR confirmaram ajustes de gama e DDC/CI. [Contexto sanitizado](../artifacts/performance/final_031_contexto.json).

Foram repetidos os cenários de 60 s, a cada 2 s, sem compilação nem download durante a coleta. O coletor produziu 58,725 s na bandeja e 58,267 s no painel. Os auxiliares foram identificados separadamente. A soma das médias residentes é apenas uma referência: páginas compartilhadas podem ser contadas mais de uma vez. [Resumo](../artifacts/performance/final_031_resumo.json).

| Cenário e processo | CPU média | Memória residente média | Memória privada média |
| --- | ---: | ---: | ---: |
| Bandeja, principal | 0,00715% | 29,770 MiB | 3,676 MiB |
| Bandeja, auxiliar de telas | 0,00239% | 26,239 MiB | 3,078 MiB |
| Painel, principal | 0,00231% | 29,683 MiB | 3,505 MiB |
| Painel, auxiliar de telas | 0,00463% | 26,217 MiB | 3,002 MiB |
| Painel, janela de configurações | 0,29631% | 122,368 MiB | 150,107 MiB |
| Bandeja, DWM do desktop inteiro | 6,88141% | 63,153 MiB | 240,516 MiB |
| Painel, DWM do desktop inteiro | 7,55747% | 63,511 MiB | 240,762 MiB |

Comparada à referência 0.2.9, a bandeja mostrou CPU baixa em ambas as versões e mais memória residente ao incluir o novo auxiliar. O isolamento impede um driver de monitor de prender o processo principal, com esse custo de memória explícito. O painel em primeiro plano mostrou mais CPU e memória na nova coleta. Uma confirmação de 30 s, após retirar o ponteiro dos controles, ainda observou 0,16385% de CPU e 123,237 MiB na janela; não há evidência suficiente para atribuir a diferença exclusivamente a uma mudança de código ou prometer redução percentual. O conteúdo de outros aplicativos e o estado do desktop não foram isolados entre versões.

Não houve motor de GPU informado para o principal ou auxiliar. No painel, os contadores informados ficaram em zero na precisão observada. O DWM apresentou aproximadamente 9,92% e 8,38% no motor 3D, pertencentes ao desktop inteiro. A coleta da bandeja registrou uma falha pontual de GPU e uma de DWM; valores ausentes não foram convertidos em zero. O coletor consumiu 6,14062 s e 7,625 s de CPU, respectivamente. Dados brutos: [bandeja](../artifacts/performance/final_031_bandeja_20261002T174112097Z_11496.json), [painel](../artifacts/performance/final_031_painel_20261002T174251535Z_11496.json), [confirmação](../artifacts/performance/final_031_painel_idle_confirm_20261002T174531287Z_11496.json).

A captura pontual da câmera concluiu em 2,0636 s, com 0,09375 s de CPU observada e pico residente de 30,531 MiB, leitura válida e saída 0. Não foram salvos quadros ou identificadores. As capturas pontuais de versões diferentes não representam um ensaio controlado da mesma iluminação. [Resultado](../artifacts/performance/final_031-camera.json).

Após o encerramento normal, a leitura das três curvas de 256 entradas de cada um dos dois monitores coincidiu exatamente com a leitura anterior à execução. Os snapshots de gama e DDC ficaram vazios e o marcador de recuperação foi removido. Isso confirma a restauração da base verificada do Windows nesta execução; não reconstrói uma calibração histórica que a versão antiga deixou de salvar.

O guia 0.3.1 também foi percorrido visualmente em configuração isolada: cinco etapas, identificação de webcam, consulta de sensor com feedback, conclusão salva e reabertura pelo painel. A configuração real permaneceu intacta nesse teste. [Evidência](../artifacts/performance/setup_031_ui_smoke.json). Calibração física em duas iluminações, HDR/OLED/notebooks e energia em watts/joules continuam sem validação física.
