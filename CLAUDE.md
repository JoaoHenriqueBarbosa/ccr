# Filosofia de Codigo Ascético

Este projeto segue uma disciplina rigorosa. Cada tipo, cada lint, cada warning existe por uma razao. Nenhum atalho. Nenhum precedente ruim. O codigo deve intimidar pela disciplina — qualquer pessoa que for escrever algo novo deve sentir o peso de manter o padrao.

## Regras Inegociaveis

### Zero Primitivos em Campos
Nenhum `String`, `bool`, `u16`, `u64`, `usize` como campo de struct ou enum. Sem excecao.

- Cada conceito de dominio tem seu newtype (`ToolUseId`, `ModelId`, `ApiKey`, `WorkingDir`, `TermRows`...)
- Bools viram enums descritivos: `RunState`, `MessageOrigin`, `Spacing`, `ToolResultStatus`, `CodeBlockState`, `TableState`, `TableRowKind`, `PendingSeparator`
- Ate campos privados internos de newtypes usam tipos dedicados (`InputText`, `ByteOffset`)
- Campos `.0` sempre privados — acesso exclusivo via metodos tipados
- Parametros de funcao tambem sao tipados: `cwd: &WorkingDir`, nao `cwd: &str`

### Tool Dispatch Tipado
- `BuiltinTool` enum com `from_name()` — match exaustivo, sem dispatch por string
- Cada tool tem seu input struct tipado (`BashInput`, `ReadInput`, `WriteInput`, `GlobInput`, `GrepInput`)
- Validacao via serde `from_value()` — sem `.get("key").and_then()` manual

### Path Traversal Guard
- `WorkingDir::validate_path()` canonicaliza e verifica que o path esta dentro do cwd
- Permite `/tmp` e `/dev/` como excecoes conhecidas
- `AppError::PathTraversal` para tentativas bloqueadas

### Typestate — Cada Fase Retorna o Proximo Tipo
- Render: `PreparedBlocks -> MeasuredBlocks -> ViewportSlice -> render()`
- Stream: `BlockAccum::Idle -> Text | Tool -> Idle`
- Timer: `TimingTimer::new() -> TimingTimer::finish() -> CompletedTimer`
  - `TurnTimer` enum wraps os 3 tipos (`Idle`, `Timing(TimingTimer)`, `Completed(CompletedTimer)`)
- Nada de retornar `self`. Cada passo retorna o tipo certo.

### Erros Tipados
- `thiserror` com enum `AppError` — nunca `anyhow`, nunca strings de erro
- `type Result<T> = std::result::Result<T, AppError>`
- `AppError::PathTraversal` para seguranca de paths

### Clippy Pedantic Zero
- `[lints.clippy]` no `Cargo.toml`: `all = "deny"`, `pedantic = "deny"` — zero warnings, zero errors
- `#[allow(...)]` apenas com `reason = "..."` justificativa explicita
- Patterns: `let...else`, `map_or`, `u16::from(bool)`, pass-by-value para Copy types

### `#[must_use]` em Tudo Que Retorna Valor
- Tipos: `ToolResultStatus`, `TermRows`, `TokenCount`, `ScrollOffset`, `AppError`, `ShortModelName`, `RequestId`, `ApiUrl`, `MaxTokens`, `ToolOutput`...
- Metodos puros: `is_error()`, `is_empty()`, `is_streaming()`, `value()`, `elapsed_ms()`...
- Evitar `#[must_use]` duplo: se o struct ja tem `#[must_use]`, metodos retornando `Self` nao precisam

### `Display` ao Inves de Ad-hoc
- Implementar `fmt::Display` — nunca `.display() -> String` ou `.as_str()` + `format!()`
- Macro `impl_string_newtype!` em `types/mod.rs` gera `Display` + `AsRef<str>` para todo newtype de String
- Todo novo `newtype(String)` DEVE ser adicionado a invocacao da macro
- Newtypes locais a um modulo (ex: `GitBranch`, `Username`) usam macro local equivalente

### Funcoes Curtas
- Maximo ~100 linhas (enforced por clippy)
- Extrair helpers com nomes descritivos
- Structs com state machines (ex: `MarkdownRenderer`) em vez de funcoes gigantes

### Arc Para Estado Compartilhado
- `Arc<AnthropicClient>` — nunca clonar o client, sempre `Arc::clone`

### SSE UTF-8 Safety
- Buffer SSE acumula `Vec<u8>` brutos — conversao para String apenas apos `\n\n` completo
- Previne corrupcao quando chunk TCP corta codepoint multi-byte

## Arquitetura

- **Duas threads**: UI (ratatui + crossterm 60fps) e backend (tokio::spawn para API + tools)
- **Canal tipado**: `mpsc::UnboundedChannel<BackendEvent>` — backend nunca bloqueia UI
- **Render pipeline typestate**: tipos enforced pelo compilador, nenhum passo pode ser pulado
- **Auth isolado**: `auth.rs` resolve API key de env/OAuth/credentials — `main.rs` limpo
