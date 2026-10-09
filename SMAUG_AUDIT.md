# Auditoria de estabilidade no Windows

Data: 2026-10-09. Escopo: inicialização, recuperação, encerramento, erros e fluxos alterados na versão 0.3.2. Não constitui uma auditoria integral das funcionalidades existentes.

Nenhum achado aberto no código desse escopo.

- `src/launcher.rs:17`: recuperação limitada a três reinícios, inicialização limitada a 45 segundos e encerramento a 30 segundos; pedidos manuais e de encerramento permanecem disponíveis durante a inicialização.
- `src/launcher_progress.rs:22`: abertura manual lenta informa progresso e permite cancelar.
- `src/status.rs:140`: o painel continua recebendo eventos entre reinícios e novas sessões.
- `src/logging.rs:8`: registros por processo com rotação e captura antecipada de panics.
- `scripts/Test-EstelRuntime.ps1:201`: configuração temporária, limite de espera e restauração do registro e ambiente mesmo quando a limpeza falha.

Verificação local: `cargo fmt --all -- --check`, `cargo check --all-targets --locked`, `cargo clippy --all-targets --locked -- -D warnings` e `cargo test --all-targets --locked` passaram; 136 testes passaram. O teste de integridade do instalador publicado passou separadamente. `smaug check --json`: 1485 arquivos, nenhuma violação.

Validação operacional: [CI 37945593055](https://github.com/DenisCDev/estel/actions/runs/37945593055), commit `3791511ed2d087b4be19afe900fa38bdfdae4cd6`, passou integralmente. O executável de produção confirmou abertura duplicada, recuperação de crash, painel único atualizado entre sessões, encerramento, limite de três reinícios, progresso e cancelamento com início oculto e preferências preservadas. Instalação, atualização do caminho, preservação do início automático desligado e desinstalação passaram. Mesa 26.2.4 com SHA256 fixo fornece renderização apenas à cópia de teste do CI.

A causa da ausência de inicialização no último login ainda não foi determinada, pois não havia registro dessa tentativa. A reinicialização real do PC não foi executada durante a validação.
