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

Não há comparação executada contra a versão nova neste registro. A compilação local com Rust 1.96.0 parou por bloqueio de uma DLL procedural em Code Integrity, apesar de o compilador ter sido aceito. [Diagnóstico sanitizado](../artifacts/performance/rust_196_code_integrity.json). A CI deve validar o código e produzir um binário identificável; depois ainda é necessário executar os mesmos cenários para afirmar qualquer ganho. Um menor uso de CPU, ou escurecimento por overlay, não prova por si só menor consumo elétrico da tela.
