# protonhax 🚀

CLI-программа помогающая запускать любые программы в контексте запущенной игры под Steam Proton, вдохновлённая скриптом [jcnils/protonhax](https://github.com/jcnils/protonhax).

## ✨ Возможности

- `init %command%` — перехват запуска игры от Steam и сохранение контекста.
- `ls` — список текущих игр (AppID и название), для которых сохранён контекст.
  - добавьте `-l` для подробностей (префикс, путь установки, Proton и время старта).
- `ls --json` — тот же список в JSON для скриптов и интеграций.
- `run <target> <cmd>` — запустить Windows‑программу через Proton в контексте игры.
  - `--detach` запускает программу в фоне с отключёнными стандартными потоками.
  - `--cwd <path|exe-dir>`, `--env NAME=VALUE` и `--unset-env NAME` настраивают запуск.
- `cmd <target>` — запустить `cmd.exe` в том же префиксе Proton.
- `exec <target> <cmd>` — запустить нативную Linux‑команду с окружением игры.
  - поддерживает повторяемые `--env NAME=VALUE` и `--unset-env NAME`.
- `env <target>` — безопасно вывести сохранённое окружение.
- `info <target> [--json]` — подробная информация об одном активном контексте.
  - `target` может быть: `appid`, `latest`, или часть имени игры.
- `doctor [--json] [--fix]` — проверка окружения и runtime‑контекстов; `--fix` удаляет stale-сессии и исправляет права.
- `profile` — сохранить, запустить, вывести или удалить профиль команды.
- `completions <shell>` — генерация автодополнений (bash/zsh/fish/powershell).

## 📦 Установка

Скачайте архив для своей системы со страницы [GitHub Releases](https://github.com/hexquant/protonhax/releases). Для большинства дистрибутивов Linux подойдёт архив `x86_64-unknown-linux-gnu`, а `x86_64-unknown-linux-musl` не зависит от системной glibc (должно работать на большем дистрибутиве Linux).

Распакуйте скачанный архив и установите бинарник:

```sh
tar -xzf protonhax-*-x86_64-unknown-linux-*.tar.gz
install -Dm755 protonhax-*-x86_64-unknown-linux-*/bin/protonhax ~/.local/bin/protonhax
```

Или соберите и установите `protonhax` из исходников (требуется Rust toolchain):

```sh
git clone https://github.com/hexquant/protonhax.git
cd protonhax
cargo install --path . --locked --root ~/.local
```

Убедитесь, что `~/.local/bin` в `PATH`.

## 🕹️ Использование со Steam

В свойствах игры → Launch Options пропишите полный путь к установленному бинарнику:

```sh
/home/<user>/.local/bin/protonhax init %command%
# или:
/home/<user>/.local/bin/protonhax init %COMMAND%
```

## 💡 Примеры CLI

Список активных игр:

```sh
protonhax ls
# или подробный вывод: AppID, название, префикс, путь установки и Proton
protonhax ls -l
```

Пример подробного вывода:

```text
1217060  Gunfire Reborn  started 12m ago
  Prefix: /home/user/.local/share/Steam/steamapps/compatdata/1217060/pfx
  Install: /home/user/.local/share/Steam/steamapps/common/Gunfire Reborn
  Proton: /home/user/.local/share/Steam/steamapps/common/Proton - Experimental/proton
```

Запустить Windows‑программу (например, трейнер) в контексте игры c appid `1217060`:

```sh
protonhax run 1217060 "/home/<user>/Downloads/Gunfire_Reborn_v1.0-v20251025_Plus_8_Trainer.exe"

# Быстрый запуск в контексте последней активной игры
protonhax run latest "/home/<user>/Downloads/trainer.exe"

# Поиск по части имени игры
protonhax run "gunfire" "/home/<user>/Downloads/trainer.exe"

# Запустить trainer в фоне из его собственного каталога
protonhax run latest --detach --cwd exe-dir "/home/<user>/Downloads/trainer.exe"

# Переопределить окружение конкретного запуска
protonhax run latest --env WINEDEBUG=-all --unset-env DXVK_LOG_LEVEL "/home/<user>/Downloads/trainer.exe"
```

Открыть `cmd.exe` в том же префиксе Proton:

```sh
protonhax cmd latest
```

Запустить нативную команду Linux с тем же окружением:

```sh
protonhax exec "gunfire" env | sort

# Переопределения доступны и для нативных команд
protonhax exec latest --env MANGOHUD=1 --unset-env WINEDEBUG mangohud --help
```

Посмотреть окружение или информацию о контексте:

```sh
protonhax env latest
protonhax info latest
```

Проверить окружение и сохранённые контексты:

```sh
protonhax doctor
# Удалить устаревшие runtime-сессии и исправить права доступа
protonhax doctor --fix
```

## 💾 Профили команд

Профиль сохраняет цель, тип команды, аргументы, рабочий каталог и изменения окружения. Конфигурация записывается атомарно в `$XDG_CONFIG_HOME/protonhax/profiles.json` с приватными правами; без `XDG_CONFIG_HOME` используется `~/.config/protonhax/profiles.json`.

```sh
# Сохранить Windows-трейнер
protonhax profile add trainer latest --detach --cwd exe-dir --env WINEDEBUG=-all -- "/home/<user>/Downloads/trainer.exe"

# Запустить сохранённый профиль; дополнительные аргументы добавляются в конец
protonhax profile run trainer -- --silent

# Сохранить нативную команду
protonhax profile add game-env latest --kind native -- env

protonhax profile ls
protonhax profile ls --json
protonhax profile remove trainer
```

Полная справка:

```sh
protonhax --help
protonhax run --help
```

## 🧩 Автодополнение

Сгенерировать автодополнения:

```sh
# Bash
protonhax completions bash > ~/.local/share/bash-completion/completions/protonhax

# Zsh
protonhax completions zsh > ~/.zfunc/_protonhax
print -P '%F{yellow}Добавьте в ~/.zshrc: fpath+=(~/.zfunc) && autoload -Uz compinit && compinit%f'

# Fish
protonhax completions fish > ~/.config/fish/completions/protonhax.fish
```

## 🛠️ Отладка и логирование

- Включить подробные логи самого protonhax:

```sh
PROTONHAX_DEBUG=1 protonhax ls
```

Runtime-контексты публикуются атомарно в `$XDG_RUNTIME_DIR/protonhax` с правами только для текущего пользователя. Для каждого запуска хранится отдельная PID-сессия; завершённые или оставшиеся после аварии сессии не используются командами `run`/`exec` и показываются в `doctor` как stale.

Снимок окружения сохраняет Unix-значения без потерь, включая не-UTF-8 и переводы строк. В целях безопасности protonhax не сохраняет переменные, в имени которых встречаются `TOKEN`, `PASSWORD`, `SECRET`, `PRIVATE_KEY`, `ACCESS_KEY` или `API_KEY`.

- Перенаправить вывод в файл (удобно для Steam):

```sh
/home/<user>/.local/bin/protonhax init %command% &> ~/protonhax.log
```

## ⚠️ Примечания

- Сообщения вида
  `ERROR: ld.so: object '.../ubuntu12_32/gameoverlayrenderer.so' ... ELFCLASS32` —
  безвредны и исходят от Steam Overlay (32‑битная библиотека подмешивается в 64‑битный процесс).
- Если игра не стартует — временно включите `PROTONHAX_DEBUG=1` и проверьте лог.
- Для Steam Flatpak разместите бинарник protonhax в домашнем каталоге, доступном sandbox, и используйте его полный путь в Launch Options. `protonhax doctor` автоматически определит Flatpak и покажет рекомендации; запускать Steam из терминала не требуется. Для нативной команды вне sandbox можно использовать профиль с `flatpak-spawn --host`.
