# Auditoria de estabilidade no Windows

Data: 2026-10-09. Escopo: inicialização, recuperação, encerramento, erros e fluxos alterados na versão 0.3.2. Não constitui uma auditoria integral das funcionalidades existentes.

Nenhum achado aberto no código desse escopo.

- `src/launcher.rs`: recuperação limitada a três reinícios, inicialização limitada a 45 segundos e encerramento a 30 segundos; pedidos manuais e de encerramento permanecem disponíveis durante a inicialização.
- `src/launcher_progress.rs`: abertura manual lenta informa progresso e permite cancelar.
- `src/status.rs`: o painel continua recebendo eventos entre reinícios e novas sessões.
- `src/logging.rs`: registros por processo com rotação e captura antecipada de panics.
- `scripts/Test-EstelRuntime.ps1`: configuração temporária, limite de espera e restauração do registro e ambiente mesmo quando a limpeza falha.

Verificação local: `cargo fmt --all -- --check`, `cargo check --all-targets --locked`, `cargo clippy --all-targets --locked -- -D warnings` e `cargo test --all-targets --locked` passaram; 136 testes passaram. O teste de integridade do instalador publicado passou separadamente. `smaug check --json`: 1485 arquivos, nenhuma violação.

Validação operacional em andamento: executar o teste completo de recuperação no Windows isolado do CI antes da publicação. O teste local anterior cobriu recuperação, painel entre sessões, encerramento, limite de reinícios e preferências; a versão final inclui progresso e cancelamento. A causa da ausência de inicialização no último login ainda não foi determinada, pois não havia registro dessa tentativa.
