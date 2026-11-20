# Серия промптов для Antigravity: Эволюция Axiom v2 (`axiom-lang` + `Axiom.Forma` + `axiom-web`)

Этот документ содержит **4 самодостаточных промпта-этапа**, которые рекомендуется передавать в Antigravity последовательно (по одному этапу на отдельный свежий диалог, чтобы контекст не раздувался и качество архитектуры оставалось максимальным).

## Порядок выполнения и контрольные точки
1. **Этап 1** — Ускорение агента (экономия токенов 2–4×) и инкрементальный кэш Z3 по Flux-хэшу.  
   *Зачем первым:* сразу ускорит сборку/верификацию на всех следующих этапах и снизит расход токенов в нативной Axiom Studio.
2. **Этап 2** — Модернизация `axiom-lang` (Axiom v1.5): модули `use`, `const`/`enum`/`layout`, синтаксический сахар, `axiom export-abi --rust`.  
   *Зачем вторым:* убирает «ручной ассемблер» в линейной памяти и создаёт базу для модульной студии и 4-го слоя UI.
3. **Этап 3** — Четвёртый слой языка **`Axiom.Forma`** (декларативный Z3-верифицируемый UI вместо HTML+CSS, компилируемый в 1-Draw-Call `Axiom.Strata`).  
   *Зачем третьим:* задаёт чистую декларативную модель интерфейсов поверх возможностей Этапа 2.
4. **Этап 4** — Модульный рефакторинг нативной `Axiom Studio` (`capsules/studio/src/axiom_studio.ax`) на модули v1.5, авто-ABI с Rust и виджеты `Axiom.Forma`.  
   *Зачем четвёртым:* проверяет весь новый стек на реальном флагманском приложении и уменьшает размер редактируемых файлов с ~3 000 строк до 150–350 строк.

> **Совет перед запуском каждого этапа в Antigravity:**  
> Копируйте текст внутри блока ` ```markdown ... ``` ` соответствующего этапа целиком. После завершения этапа проверьте, что `git status` чист от мусора и все тесты проходят, сделайте коммит и переходите к следующему этапу в новом чате Antigravity.

---

# ПРОМПТ 1 — Этап 1: Экономия токенов агента (`axiom-web`) + инкрементальный кэш Z3 (`axiom-lang`)

```markdown
Ты работаешь над двумя связанными репозиториями Axiom:
- `/home/aminadav/Projects/axiom-web` (нативный плеер и Axiom Studio Native на Rust + wgpu + Axiom `.axw`)
- `/home/aminadav/Projects/axiom-lang` (компилятор языка Axiom: Telos + Flux + Strata, Z3-верификация, прямой эмиттер Wasm)

ОБЯЗАТЕЛЬНЫЕ ПРАВИЛА:
1. Все планы внедрения (`implementation_plan.md`) и объяснения пользователю пиши на русском языке.
2. Весь код в репозиториях, комментарии и документацию пиши на английском языке (BUSL-1.1), кроме русскоязычного UI самой локальной студии Axiom Studio.
3. Сохраняй полную обратную совместимость и все существующие тесты. Запрещены `todo!()`, `unimplemented!()`, заглушки.
4. Всегда сохраняй логи и результаты замеров (до/после).

## Цель Этапа 1
Резко снизить расход токенов и время работы ИИ-агента в нативной студии (`axiom-web/src/agent.rs`), а также ускорить повторную Z3-верификацию больших `.ax`-файлов в `axiom-lang` за счёт контент-адресуемого кэша доказательств по Flux-хэшам.

---

## Часть A: Оптимизация агентского цикла в `/home/aminadav/Projects/axiom-web/src/agent.rs`

Сначала изучи текущую реализацию `functionDeclarations`, диспетчера инструментов и агентского цикла `contents.push(...)` в `/home/aminadav/Projects/axiom-web/src/agent.rs`.

Реализуй 3 улучшения:

### A1. Новый инструмент `get_file_outline`
- Назначение: за 1 вызов и минимальное число токенов вернуть структуру файла `.ax` или `.rs` без чтения всего тела файла.
- Входной параметр: `path: String` (обязательно проверяй, что путь находится внутри `~/Projects`, как в `read_project_file`).
- Поведение:
  - Для `.ax`: извлекает все объявления `fn`, `export fn`, `const`, `enum`, `struct`, `layout`, `forma`, `use` с указанием номера начальной и конечной строки (`start_line..end_line`) и полной сигнатуры (включая эффекты `-[...]->` и краткое наличие `req`/`ens`).
  - Для `.rs`: извлекает `pub fn`/`fn`, `struct`, `enum`, `trait`, `impl`, `const`, `mod` с диапазонами строк (`start_line..end_line`).
  - Ограничивай вывод разумным лимитом (например, до 300 символов на строку, до 400 записей с пометкой, если обрезано).
- Добавь описание инструмента в `functionDeclarations`, в диспетчер выполнения и в строку живого статуса (например: `"Сканирую структуру <file>…"`).

### A2. Новый инструмент `batch_edit_project_file`
- Назначение: применить несколько независимых правок в одном файле за 1 ход модели вместо 3–5 последовательных ходов.
- Входные параметры:
  - `path: String`
  - `edits: Vec<{ target: String, replacement: String }>`
- Поведение:
  - Сначала проверяет **все** правки в памяти на актуальном содержимом файла (каждый `target` должен встречаться ровно 1 раз и не конфликтовать).
  - Если хотя бы один `target` не найден или неоднозначен — файл на диске **не изменяется** (атомарный откат), а в ответе возвращается точный индекс и причина ошибки.
  - Если все правки валидны — записывает файл один раз и возвращает сводку (`applied_count`, обновлённое число строк).

### A3. Авто-свёртка старых больших `functionResponse` в истории текущего цикла (`compact_old_tool_responses`)
- Проблема: в агентском цикле на 20–30 шагов ранее прочитанные куски кода (`read_project_file`, `search_project`, `list_project_files`, `get_file_outline`) отправляются в модель на каждом новом шаге, давая квадратичный рост входных токенов $O(N^2)$.
- Требования к реализации:
  - Перед сериализацией `contents` в тело запроса к Gemini применяй неразрушающую функцию компактификации к рабочей копии `contents` (или безопасно сворачивай старые шаги), соблюдая жёсткие инварианты протокола Gemini 4:
    1. **Никогда** не трогай сообщения `model` (особенно `thoughtSignature` и `functionCall`) и не нарушай чередование ролей `user`/`model`.
    2. Оставляй **полными** последние `4` tool-шага (последние 4 сообщения с `functionResponse`) и все ответы `edit_project_file` / `batch_edit_project_file` / `write_project_file` / `verify_axiom_file` / `run_project_command` с ошибками.
    3. Для более старых `functionResponse` от `read_project_file`, `search_project`, `list_project_files`, `get_file_outline`, если сериализованный ответ превышает `900` символов, заменяй поле `content`/`matches`/`files`/`outline` на компактную заглушку с метаданными, например:
       `"[compacted older tool output: read_project_file <path> lines <start>..<end>, original <N> chars; call read_project_file again if exact lines are needed]"`.
  - Обнови системный промпт в `agent.rs`, явно указав агенту сначала вызывать `get_file_outline` для крупных файлов (`.ax`/`.rs`) и использовать `batch_edit_project_file` при множественных правках в одном файле.

---

## Часть B: Инкрементальный дисковый кэш Z3-доказательств по Flux-хэшу в `/home/aminadav/Projects/axiom-lang`

Изучи `/home/aminadav/Projects/axiom-lang/src/store.rs`, `src/verify.rs`, `src/canon.rs` и `src/main.rs`.

### B1. Криптографически корректный ключ кэша доказательств
- Храни кэш в `.axiom/proof_cache.json` внутри рабочей директории графа `GraphStore`.
- **Критически важно для математической строгости:** кэш-ключ функции НЕ может быть просто `node.hash`, потому что при верификации вызывающей функции `verify.rs` опирается на контракты (`req`/`ens`) вызываемых функций (`load_callee`)!
- Поэтому определи стабильный `proof_key = SHA-256(...)`, включающий:
  1. константу версии верификатора (`PROOF_CACHE_SCHEMA_VERSION = "axiom-z3-v1"`),
  2. `node.hash` проверяемой канонической функции,
  3. отсортированный список `(callee_hash, callee_contract_digest)` для всех прямых вызываемых функций (где `callee_contract_digest` зависит от `params`, `ret_type`, `effects`, `requires`, `ensures` вызываемого узла),
  4. идентификатор/версию бинарника `z3` (вычисленную один раз за запуск процесса через `z3 --version`).
- Если изменилось тело вызываемой функции, но её контракт (`req`/`ens`/сигнатура) не изменился — вызывающие функции остаются валидными в кэше. Если контракт изменился — кэш автоматически промахивается и Z3 перепроверяет функцию!

### B2. Интеграция в `GraphStore` и CLI
- В `VerificationReport` добавь поле `pub cached: bool` (с `#[serde(default)]` для обратной совместимости JSON).
- В `GraphStore::verify_module` и `GraphStore::ingest_module`:
  - при наличии валидной записи в `proof_cache.json` пропускай запуск процесса `z3`, возвращая сохранённый `obligations_checked`, `elapsed_ms: 0`, `cached: true`, `status: "PROVED (cached)"`.
  - после успешного доказательства через `z3` сохраняй запись в кэш и атомарно записывай `.axiom/proof_cache.json` (через временный файл + rename).
  - **Никогда** не кэшируй ошибки / контрпримеры (`FAILED`) — кэшируются только полностью доказанные (`PROVED`) узлы.
- В `src/main.rs` добавь опциональный флаг `--no-cache` для команд `verify`, `ingest`, `compile-wasm`, который принудительно игнорирует чтение кэша и заново гоняет Z3.

---

## Тестирование и критерии приёмки
1. В `/home/aminadav/Projects/axiom-web`:
   - Напиши unit-тесты для `get_file_outline` (на реальном `.ax` и `.rs`), `batch_edit_project_file` (успешное применение + атомарный откат при ошибке), и `compact_old_tool_responses` (проверка сохранения `thoughtSignature`, последних 4 шагов и сжатия старых больших чтений).
   - Выполни `cargo test` и `cargo build --release` — все тесты должны пройти (0 warnings / 0 errors).
2. В `/home/aminadav/Projects/axiom-lang`:
   - Напиши интеграционные тесты кэша Z3:
     - первый запуск `verify` → `cached == false`,
     - второй запуск того же модуля → `cached == true`,
     - изменение контракта вызываемой функции → автоматическая инвалидация кэша вызывающей функции (`cached == false`),
     - флаг `--no-cache` → `cached == false`.
   - Выполни `cargo test` и `cargo build --release`.
3. Проведи реальный замер времени на `/home/aminadav/Projects/axiom-web/capsules/studio/src/axiom_studio.ax`:
   - первый `axiom verify` (холодный кэш, `--no-cache`) vs второй `axiom verify` (тёплый кэш) — запиши точные миллисекунды в финальный отчёт.
```

---

# ПРОМПТ 2 — Этап 2: Модернизация `axiom-lang` (Axiom v1.5: модули, `const`/`enum`/`layout`, синтаксический сахар, `export-abi --rust`)

```markdown
Ты работаешь над компилятором языка Axiom в `/home/aminadav/Projects/axiom-lang`.
Также для регрессионной проверки используй флагманскую капсулу `/home/aminadav/Projects/axiom-web/capsules/studio/src/axiom_studio.ax`.

ОБЯЗАТЕЛЬНЫЕ ПРАВИЛА:
1. Планы (`implementation_plan.md`) и объяснения пользователю пиши на русском языке.
2. Весь код, комментарии и документацию в репозитории пиши на английском языке (BUSL-1.1).
3. **100% обратная совместимость:** все существующие `.ax` файлы (`examples/*.ax`, `tests/sprint1..7`, `capsules/studio/src/axiom_studio.ax`, `capsules/m2_text_demo/src/text_demo.ax`) обязаны парситься, верифицироваться Z3 и компилироваться в Wasm без единого изменения их исходников.
4. Запрещены `todo!()`, `unimplemented!()`, ослабление проверок Z3 или обход системы эффектов.

## Цель Этапа 2
Поднять выразительность `axiom-lang` до уровня, удобного для больших UI- и системных приложений, сохранив минимализм, детерминизм, Z3-верификацию и прямую миллисекундную компиляцию в Wasm (`Axiom.Strata`).

Сначала изучи `src/lexer.rs`, `src/ast.rs`, `src/parser.rs`, `src/canon.rs`, `src/typecheck.rs`, `src/verify.rs`, `src/strata_wasm.rs`, `src/main.rs`.

---

## Что нужно реализовать

### 1. Многофайловые проекты (`use "path.ax";`)
- Добавь в синтаксис топ-уровня директиву импорта:
  `use "relative/path.ax";`
- Реализуй загрузчик графа файлов `parse_module_from_path(entry_path: &Path) -> Result<Module, AxiomDiagnostic>`:
  - Разрешает пути относительно директории текущего файла.
  - Дедуплицирует повторные `use` одного и того же канонического файла (алмазные зависимости работают чисто).
  - Обнаруживает и возвращает чёткую диагностику при циклических `use`.
  - Собирает объявления (`const`, `enum`, `layout`, `fn`) в единый `Module` в детерминированном топологическом порядке.
- Подключи `parse_module_from_path` в CLI (`verify`, `ingest`, `compile-wasm`, `export-abi`, `strata-elf` и др.).

### 2. Константы, перечисления и типизированная линейная память (`const`, `enum`, `layout`)
Это ключевая фича для уничтожения «магических адресов» (`mem_load(47)`, `500..8700`) в `Axiom.Strata`:

- **`const`:**
  ```axiom
  const MAX_INPUT_CAP: i64 = 8192;
  const INPUT_BUF_END: i64 = 500 + MAX_INPUT_CAP + 4;
  ```
  - Поддерживает целочисленные и булевы константные выражения (`+`, `-`, `*`, `/`, `%`, `<<`, `>>`, `&`, `|`, `^`, ссылки на ранее объявленные `const`).
  - Доступны везде, где ожидается выражение, включая `req`/`ens` контракты Z3 и границы `while ... max CONST`.

- **`enum`:**
  ```axiom
  enum HostAction {
    None = 0,
    SendChat = 1,
    PickImage = 11,
  }
  ```
  - Автоинкремент значений, если `= N` опущено.
  - Доступ через `HostAction::SendChat`.
  - Компилируется в `i64` без накладных расходов в Wasm, при этом тайпчекер проверяет существование enum и варианта, а Z3 видит точные целочисленные значения.

- **`layout` (типизированная карта линейной памяти `Strata`):**
  ```axiom
  layout StudioMem at 0 {
    tick: word,                  // 1 word (4 bytes, word index 0)
    theme: word,                 // word index 1
    in_scroll_y: word at 47,     // explicit pinned word offset (optional)
    input_buf: words[8200] at 500,
    projects_buf: words[4000] at 12000,
  }
  ```
  - Поля могут иметь либо автоматическое последовательное смещение, либо явное `at <const_expr>`.
  - **Статическая проверка безопасности памяти на этапе компиляции (до Z3 и Wasm):** компилятор обязан доказывать, что **ни одно поле/буфер в `layout` (и между разными `layout`) не перекрывается по диапазону слов `[offset, offset + len)`**! При малейшем нахлёсте — жёсткая ошибка компиляции `E062_LAYOUT_OVERLAP` с указанием обоих конфликтующих полей и диапазонов.
  - Доступ из кода Axiom:
    - доступ к смещению `StudioMem::field`, размеру `StudioMem::field::len`, концу `StudioMem::field::end`, а также сахар чтения/записи для `word`-полей (`StudioMem.in_scroll_y` / `StudioMem.in_scroll_y = val`), автоматически требующий эффект `Mem`!

### 3. Синтаксический сахар с нулевой ценой в Wasm (desugaring в AST / перед канонизацией)
1. **Символьные литералы:** `'a'`, `'#'`, `' '`, `'\n'`, `'\t'`, `'\r'`, `'\\'`, `'\''` → `Expr::IntLit(codepoint as i64)`.
2. **`else if`:** цепочки `if a { ... } else if b { ... } else { ... }` без лишних фигурных скобок.
3. **`if` без `else` в позиции statement:** `if cond { do_something(); }` автоматически дешугарится в `if cond { do_something(); } else { () }`.
4. **Составные присваивания:** `x += e;`, `x -= e;`, `x *= e;`, `x |= e;`, `x &= e;` для `let mut` переменных (дешугаринг в `x = x <op> (e);`).
5. **`match` по `i64` / `enum` / `bool`:**
   ```axiom
   match action {
     HostAction::None => 0,
     HostAction::SendChat | HostAction::PickImage => 1,
     _ => -1,
   }
   ```
   - Дешугарится в чистое дерево `if/else` (с однократным вычислением scrutinee в временную переменную), благодаря чему автоматически и точно поддерживается и в `verify.rs` (Z3), и в `strata_wasm.rs`, и в `codegen.rs`.
   - Проверь обязательность исчерпывающей ветки `_` (или всех вариантов `enum`).

### 4. Генератор общего ABI для Rust: `axiom export-abi <entry.ax> --rust -o <out.rs>`
- Добавь новую CLI-команду `export-abi <file.ax> --rust -o <out.rs>` (и опционально `--json`).
- Из всех `const`, `enum` и `layout` модуля (включая подключённые через `use`) генерирует чистый, отформатированный, самодокументируемый Rust-файл с заголовком `// @generated by axiom export-abi — DO NOT EDIT MANUALLY`:
  - `pub const ...: i64 = ...;` (и `usize` для word-смещений/размеров полей `layout`: `pub const STUDIO_MEM_INPUT_BUF_WORD: usize = 500;`, `pub const STUDIO_MEM_INPUT_BUF_LEN: usize = 8200;`),
  - `#[repr(i32)] pub enum HostAction { ... }` + `TryFrom<i32>`,
  - статические `const _: () = assert!(...);`, подтверждающие непересечение буферов.

---

## Тестирование и критерии приёмки
1. Создай полноценный набор тестов `tests/sprint8_modern_syntax_tests.rs`, покрывающий:
   - многофайловый `use` (включая алмазный импорт и отлов циклов),
   - `const` в выражениях, контрактах Z3 и `while max`,
   - `enum` и `match` (включая Z3-доказательство пост-условий через `match` и компиляцию в Wasm с реальным исполнением),
   - `layout` (авто-раскладка, явные `at`, ошибка `E062_LAYOUT_OVERLAP` при пересечении, чтение/запись в Wasm-памяти),
   - символьные литералы, `else if`, `if` без `else`, `+=`/`-=`,
   - `export-abi --rust` (проверка, что сгенерированный Rust-код валиден и компилируется `rustc --crate-type lib`).
2. Прогони все старые и новые тесты: `cargo test` в `/home/aminadav/Projects/axiom-lang` — 100% passed.
3. Регрессионная проверка на реальной студии:
   - `target/release/axiom verify /home/aminadav/Projects/axiom-web/capsules/studio/src/axiom_studio.ax`
   - `target/release/axiom compile-wasm /home/aminadav/Projects/axiom-web/capsules/studio/src/axiom_studio.ax -o /tmp/axiom_studio_check.axw`
   - `/home/aminadav/Projects/axiom-web/target/release/axiom-web --headless-check /tmp/axiom_studio_check.axw` → `HEADLESS_OK`.
```

---

# ПРОМПТ 3 — Этап 3: Четвёртый слой языка `Axiom.Forma` (декларативный Z3-верифицируемый UI вместо HTML+CSS)

```markdown
Ты проектируешь и реализуешь четвёртый архитектурный слой языка Axiom — **`Axiom.Forma`** — внутри `/home/aminadav/Projects/axiom-lang` (с проверкой рендера в `/home/aminadav/Projects/axiom-web`).

Архитектура четырёх слоёв Axiom:
1. `Axiom.Telos` — намерение и Z3-контракты (`req`/`ens`, формальные доказательства).
2. `Axiom.Flux` — детерминированная логика, эффекты и content-addressed граф.
3. `Axiom.Forma` — **пространственно-интерактивный UI-слой** (замена HTML + CSS + ручного hit-testing).
4. `Axiom.Strata` — линейная память, единый 1-Draw-Call GPU-поток примитивов и прямой байткод Wasm.

ОБЯЗАТЕЛЬНЫЕ ПРАВИЛА:
1. Планы (`implementation_plan.md`) и объяснения пользователю пиши на русском языке.
2. Весь код, комментарии и спецификации в репозитории пиши на английском языке (BUSL-1.1).
3. Никакого DOM, CSS-каскада, JavaScript или утяжеления рантайма `axiom-web`: `Axiom.Forma` является **компилируемым фронтендом**, который понижается (lowers) в чистые функции `Axiom.Flux`/`Axiom.Strata`, пишущие в существующий 1-Draw-Call буфер примитивов (`push_prim` / `set_clip_rect`) с нулевыми накладными расходами.
4. Запрещены `todo!()`, `unimplemented!()` и фейковые «успешные» проверки Z3.

---

## Сначала изучи текущий рендер-протокол `axiom-web`
Прочитай начало `/home/aminadav/Projects/axiom-web/capsules/studio/src/axiom_studio.ax` (функции `set_clip_rect`, `push_prim`, `ui_rect`, `ui_text`, `ui_line`) и `/home/aminadav/Projects/axiom-web/src/gpu.rs` / `src/shader.wgsl`, чтобы формат GPU-примитивов, генерируемый `Axiom.Forma`, был **на 100% нативно совместим** с `axiom-web`.

---

## Что нужно спроектировать и реализовать

### 1. Инженерная спецификация `/home/aminadav/Projects/axiom-lang/docs/AXIOM_FORMA_SPEC.md`
Короткая и чёткая спецификация:
- грамматика декларативных блоков `forma`,
- модель раскладки (2-проходная: `measure` / `arrange` без рекурсивных циклов — детерминированная за 1 кадр),
- единый источник правды для Layout + Clipping + Hit-Test + Render + Z-слоёв (`Overlay`),
- формальный список Z3-инвариантов UI (`Telos × Forma`).

### 2. Синтаксис и AST `Axiom.Forma` (`src/forma/`)
Создай модульную подсистему `src/forma/` (`mod.rs`, `ast.rs`, `parser.rs`, `verify_ui.rs`, `lower.rs`):

Поддержи декларативные UI-компоненты прямо внутри `.ax`-файлов (или подключаемых через `use`):
```axiom
forma ComposerBar(x: i64, y: i64, w: i64, h: i64, theme_bg: i64, border_col: i64)
  req w >= 240 && h >= 80
  -[Mem]-> i64
{
  Box(x: x, y: y, w: w, h: h, radius: 12, border: 1, bg: theme_bg, border_col: border_col, pad: 10) {
    Col(gap: 8, grow: 1) {
      Scroll(id: 3, grow: 1, clip: 3) {
        Slot(render: draw_input_text, on_wheel: handle_input_wheel)
      }
      Row(h: 32, gap: 8, align: center) {
        Button(id: 101, w: 32, h: 32, radius: 8, on_click: 11) {
          Slot(render: draw_attach_icon)
        }
        Dropdown(id: 102, w: 170, h: 30, radius: 8, on_click: 12) {
          Slot(render: draw_model_label)
        }
        Spacer(grow: 1)
        Button(id: 103, w: 34, h: 34, radius: 17, on_click: 1) {
          Slot(render: draw_send_icon)
        }
      }
    }
  }
}
```

Обязательный базовый набор примитивов `Forma`:
- Контейнеры раскладки: `Box`, `Col`, `Row`, `Stack`, `Scroll`, `Overlay` (гарантированно рисуется в отложенном верхнем проходе поверх всего дерева и получает приоритетный hit-test!).
- Интерактивные и листовые узлы: `Button`, `Dropdown`, `Spacer`, `Divider`, `Rect`, `Slot` (мост для вызова кастомных `Axiom.Flux`-функций вроде векторной иконки, Markdown-рендера или текстового поля с курсором, получающих уже вычисленные компилятором `rx, ry, rw, rh, clip_id`!).
- Свойства: `x`, `y`, `w`, `h`, `min_w`, `min_h`, `grow`, `pad`, `pad_x`, `pad_y`, `gap`, `align` (`start | center | end | stretch`), `bg`, `hover_bg`, `border`, `border_col`, `radius`, `shadow`, `clip`, `id`, `on_click`, `visible`.

### 3. Z3-верификация интерфейса (`src/forma/verify_ui.rs`) — главная фича Axiom.Forma
Для каждого блока `forma` компилятор строит SMT-модель геометрии узлов (при выполнении `req` блока) и **реально доказывает через Z3**:
1. **Non-degeneracy (невырожденность):** при выполнении `req` все видимые интерактивные элементы (`Button`, `Dropdown`, `Scroll`) имеют `rw > 0 && rh > 0` (не схлопываются в 0 пикселей).
2. **No sibling overlap (отсутствие перекрытия соседей):** внутри `Row` и `Col` прямоугольники соседних дочерних элементов не пересекаются (`a.x + a.w <= b.x` для `Row`, `a.y + a.h <= b.y` для `Col`).
3. **Bounds & Clip containment (вложенность клиппинга):** дочерние элементы без `Scroll` не выходят за внутренние границы родительского контейнера (при `pad` и фиксированных размерах) и их `clip_rect` находится внутри родительского `clip_rect`.
4. **Primitive budget bound:** статическая верхняя оценка числа GPU-примитивов, излучаемых деревом `forma`, строго меньше лимита буфера (`80_000`).

Если пользователь (или ИИ) написал `Row(w: 200)` с тремя кнопками по `80px` и `gap: 10` (итого `260 > 200`) — Z3 обязан поймать это **на этапе компиляции** с понятным контрпримером (`E090_FORMA_LAYOUT_VIOLATION`)!

### 4. Понижение (`src/forma/lower.rs`) в чистые функции `Axiom.Flux` / `Axiom.Strata`
- Каждый `forma Name(...)` понижается в обычные `FnDef` в `Module` (перед `canon`/`typecheck`/`verify`/`strata_wasm`), поэтому автоматически участвует в `Flux`-дедупликации, кэше Z3 из Этапа 1, паспорте безопасности и прямом эмиттере Wasm!
- Генерируется единая согласованная логика:
  - расчёт прямоугольников (`rx, ry, rw, rh`) по `Col`/`Row`/`Stack`/`grow`/`pad`/`gap`/`align`,
  - hit-testing по текущей позиции мыши (подсветка `hover_bg` + возврат `clicked_id` при клике, с приоритетом `Overlay`),
  - вызовы `set_clip_rect` и `push_prim` (совместимо с `axiom-web` ABI).

### 5. Эталонная демонстрационная капсула и сквозная проверка
- Создай `/home/aminadav/Projects/axiom-lang/examples/forma_ui_demo.ax` — полноценное мини-приложение на `Axiom.Forma` (боковая панель, заголовок, `Col`/`Row`, кнопки с `hover_bg`, `Scroll`-область, `Dropdown` с `Overlay`).
- Напиши тесты `tests/sprint9_forma_tests.rs`:
  1. Успешная Z3-верификация корректного `forma`-дерева.
  2. Намеренная ошибка переполнения `Row` → Z3 возвращает `E090_FORMA_LAYOUT_VIOLATION` с контрпримером.
  3. Намеренное схлопывание кнопки в `w <= 0` → Z3 ловит ошибку на этапе компиляции.
  4. Компиляция `examples/forma_ui_demo.ax` в `.axw` (`axiom compile-wasm`) + проверка headless-запуска через `/home/aminadav/Projects/axiom-web/target/release/axiom-web --headless-check` → `HEADLESS_OK` с реальными примитивами (`Prims > 0`) и 1 Draw Call.
- Прогони `cargo test` и `cargo build --release` в `/home/aminadav/Projects/axiom-lang` — все тесты (sprint 1..9) зелёные.
```

---

# ПРОМПТ 4 — Этап 4: Модульный рефакторинг нативной Axiom Studio (`axiom_studio.ax`) на Axiom v1.5 + `Axiom.Forma` + авто-ABI

```markdown
Ты выполняешь архитектурный рефакторинг флагманской капсулы нативной Axiom Studio в `/home/aminadav/Projects/axiom-web` с использованием новых возможностей компилятора `/home/aminadav/Projects/axiom-lang` (Этапы 1–3: кэш Z3, модули `use`, `const`/`enum`/`layout`, сахар `match`/`else if`/символы, `axiom export-abi --rust` и 4-й слой UI `Axiom.Forma`).

ОБЯЗАТЕЛЬНЫЕ ПРАВИЛА:
1. Планы (`implementation_plan.md`) и объяснения пользователю пиши на русском языке.
2. Весь код в репозитории и комментарии пиши на английском языке (BUSL-1.1), сохраняя 100% русскоязычного интерфейса самой Axiom Studio Native.
3. **Нулевая потеря функциональности:** все фичи нативной студии обязаны работать так же или лучше, а все 17+ существующих тестов `cargo test` в `axiom-web` обязаны пройти без ослабления проверок.
4. Запрещены `todo!()`, `unimplemented!()` и удаление существующих тестов.

## Цель Этапа 4
Превратить монолитный файл `/home/aminadav/Projects/axiom-web/capsules/studio/src/axiom_studio.ax` (~2 971 строка, ручные адреса памяти и ручная геометрия) в чистую модульную кодовую базу (8–10 файлов по ~150–350 строк) с:
1. единым типизированным ABI между Axiom и Rust (через `layout`/`enum`/`const` и `axiom export-abi --rust`),
2. декларативными Z3-верифицируемыми экранами/панелями на `Axiom.Forma`,
3. простым скриптом пересборки капсулы + ABI.

---

## Пошаговый план рефакторинга

### Шаг 1. Проектирование единого `abi.ax` и генерация `src/studio_abi.rs`
- Создай `/home/aminadav/Projects/axiom-web/capsules/studio/src/abi.ax`:
  - перенеси всю карту памяти (`0..63`, `400..463`, `500..8700`, `12000..16000`, `25000..28600`, `28700..28999`, `29000..31800`, `32000..36150`, `36200..36400`, `36500..36800`, `48000..48063`, `65536..705536`, `710000..743000`, `750000..751023`, `752000..2050000`, `2052000..2085000`) в строгий `layout StudioMem`, доказанно непересекающийся на этапе компиляции!
  - оформи все коды действий хоста (`HostAction`), режимы просмотра (`ViewMode`), валюты (`Currency`), модели (`ModelId`), типы скроллбаров (`DragScrollbar`), ID клип-прямоугольников (`ClipId`) в `enum` и `const`.
- Сгенерируй `/home/aminadav/Projects/axiom-web/src/studio_abi.rs` командой:
  `/home/aminadav/Projects/axiom-lang/target/release/axiom export-abi capsules/studio/src/abi.ax --rust -o src/studio_abi.rs`
- Подключи `mod studio_abi;` в `src/main.rs` (и `src/lib.rs`, если есть) и замени захардкоженные числовые адреса/коды `host_action` в `src/runtime.rs`, `src/capabilities.rs` и `src/main.rs` на константы из `studio_abi`.

### Шаг 2. Разбиение `axiom_studio.ax` на чистые модули (через `use`)
Разбей монолит на логические модули в `/home/aminadav/Projects/axiom-web/capsules/studio/src/`:
1. `abi.ax` — память (`layout`), константы, `enum`.
2. `primitives.ax` — `Telos`-контракты (`clamp_i64`, `clamp_byte`, `sel_lo`, `sel_hi`, `point_in_rect`, `pack_rgba`), `set_clip_rect`, `push_prim`, `ui_rect`, `ui_line`, базовый рендер глифов/текста.
3. `theme.ax` — палитра тёмной/светлой темы и масштабирование шрифта (`font_scale_pct`).
4. `icons.ax` — все векторные иконки (`ui_icon_chevron_down`, `ui_icon_image`, иконки настроек, копирования, отправки, стопа и т.д.).
5. `text_buf.ax` — работа с UTF-32 буферами, переносом строк, выделением, курсором, навигацией, буфером обмена (`axw_on_paste_text`, `axw_on_copy_text`).
6. `markdown.ax` — парсер и рендер Markdown + блоков кода с использованием символьных литералов (`'#'`, `` '`' ``, `'*'`, `' '`, `'\n'`) вместо голых ASCII-чисел!
7. `sidebar.ax` — левая панель (кнопки, дерево проектов `~/Projects`, список сохранённых диалогов, скроллбар сайдбара, `⚙ Настройки`, RAM).
8. `feed.ax` — основная лента сообщений, выделение подстроки мышью, телеметрия, копирование ответа, Web Clean-Reader.
9. `composer.ax` — поле ввода на 8 192 символа с прокруткой (`in_scroll_y`), скроллбаром, превью прикреплённой картинки (`pasted_img_w/h`), кнопкой выбора файла, селектором модели Gemini и кнопкой отправки/стопа.
10. `modals.ax` — окно `⚙ Настройки`, модалка истории Antigravity, выпадающие списки (модель, валюта), контекстное меню ПКМ.
11. `axiom_studio.ax` — тонкая точка входа (`use "abi.ax"; ...`), содержащая только связывание `init_app`, `step_ui` и экспорты капсулы (сохрани имя `axiom_studio.ax` для полной совместимости со всеми скриптами и тестами!).

### Шаг 3. Перевод ключевых панелей на `Axiom.Forma`
Переведи как минимум 3 ключевые UI-зоны на декларативные Z3-верифицируемые блоки `forma`:
- **`composer.ax`:** карточка поля ввода (превью картинки + прокручиваемое текстовое поле + нижняя панель с кнопкой прикрепления, селектором модели с шевроном, счётчиком стоимости и кнопкой отправки/стопа).
- **`modals.ax`:** окно `⚙ Настройки` и выпадающие списки (через `Overlay`, чтобы Z3 гарантировал порядок слоёв и отсутствие перекрытий элементов внутри модалки).
- **`sidebar.ax`:** каркас боковой панели (шапка, кнопки, прокручиваемое дерево проектов, нижний футер с `⚙ Настройки` и RAM).

При рефакторинге очисти код от глубоких «лесенок» `if/else` за счёт `else if`, `match`, `+=` и символьных литералов.

### Шаг 4. Скрипт сборки капсулы и полная проверка
- Добавь (или обнови) скрипт `/home/aminadav/Projects/axiom-web/scripts/build_studio.sh`, который за одну команду:
  1. запускает `axiom export-abi capsules/studio/src/abi.ax --rust -o src/studio_abi.rs`,
  2. запускает `axiom verify capsules/studio/src/axiom_studio.ax` (проверяет Z3 + `Axiom.Forma` контракты),
  3. компилирует `axiom compile-wasm capsules/studio/src/axiom_studio.ax -o capsules/studio/axiom_studio.axw`,
  4. собирает `cargo build --release`,
  5. прогоняет `cargo test` и `target/release/axiom-web --headless-check capsules/studio/axiom_studio.axw`.
- Выполни полную проверку:
  - `axiom verify` (первый и повторный запуск с кэшем Z3 — зафиксируй ускорение в логе),
  - `cargo test` в `/home/aminadav/Projects/axiom-web` — 100% passed (все 17+ тестов, включая `studio_capsule_selects_and_copies_chat_message_substring`, `studio_capsule_composer_scroll_image_and_chevron`, SSE-тесты агента),
  - `target/release/axiom-web --headless-check capsules/studio/axiom_studio.axw` → `HEADLESS_OK`,
  - проверь размер `.axw` и число строк по модулям (подтверди, что самый большой `.ax` модуль стал в разы компактнее прежних 2 971 строк).
```
