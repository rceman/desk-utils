# Ревью через `airelay review_sol`

Живая Sol-сессия уже запущена — **не создавать новых профилей**.

## Сессия

Старт была через:

```
airelay start codex --key review_sol --workspace review.sol -- resume <session-id>
```

Дальше работа идёт через `attach` к живой сессии (tmux-подобная TTY-сессия),
в которую можно вставлять текст и читать вывод.

## Workflow

1. **Подготовить review-файл** в `/home/therceman/agent-workspaces/review/agents/sol/`
   — e.g. `scrollshot_grip.md`, `scrollshot_glitch.md`.
   Содержимое: описание проблемы + вставки кода (классы/функции целиком).

2. **Отправить промпт в сессию** через `write_to_process` + `<CR>`:

   ```
   New task: <path-to-review-file> — <что проверить>.
   Report bugs with file/line refs. APPEND verdict (READY|NEEDS_REWORK)
   and findings at the END of the file.
   ```

3. **Ждать** — Sol работает 5–15 минут. Проверять рост файла
   (`wc -l`) и/или читать вывод сессии (`get_output`).

4. **Вердикт дописан в конец файла** — читать tail файла, там
   `Verdict:` + findings + готовый код-патч, если требуется.

5. **Применить** код из файла в `scrollshot.py`, прогнать
   `ast.parse` (syntax), задеплоить (`cp` в `C:\Tools\diskhound\...`),
   перезапустить приложение, закоммитить и запушить.

## Заметки

- Sol пишет **в конец файла** — всегда просить append, иначе
  ответ уйдёт только в чат сессии.
- Если в файле есть вставки кода — Sol матчит их с реальными
  файлами и даёт рекомендации с file:line.
- Полноэкранный Border-оверлей и диагноз глюка на старте записи
  пришли именно через этот цикл.
- Для нескольких независимых вопросов — отдельные review-файлы,
  промпт отправляется в ту же сессию.

## Реальные использованные файлы

- `scrollshot_review.md` — два полных ревью (10 находок P1/P2)
- `scrollshot_grip.md` — редизайн Border → единый оверлей
- `scrollshot_glitch.md` — диагноз артефакта старта записи
- `scrollshot_md.md` — дизайн колбаса→markdown конвейера
