# ProtonHax 🚀

[🇺🇸 English](./README.md) · [🇷🇺 Русский](./README.ru.md)

**Запускайте Windows-программы в окружении Proton уже запущенной игры Steam.**

ProtonHax — это инструмент командной строки для Linux, написанный на Rust. Он запускает Windows-программы — например, трейнеры, Cheat Engine и средства отладки — в том же префиксе и окружении Proton, что и работающая игра Steam. Проект вдохновлён скриптом [jcnils/protonhax](https://github.com/jcnils/protonhax).

ProtonHax сохраняет окружение Steam Proton запущенной игры, чтобы затем вы могли:

- Запускать `.exe`-файлы в префиксе Proton этой игры
- Запускать Cheat Engine, трейнеры и средства отладки через Proton
- Открывать `cmd.exe` в окружении Proton запущенной игры
- Выполнять нативные Linux-команды с переменными окружения игры
- Находить запущенные игры по Steam AppID или названию
- Диагностировать повреждённые или устаревшие контексты Proton

## ✨ Возможности

- `init %command%` — перехватить запуск игры из Steam и сохранить её контекст.
- `ls` — вывести активные игры (AppID и название), для которых сохранён контекст.
  - Добавьте `-l`, чтобы увидеть префикс, путь установки, исполняемый файл Proton и время запуска.
- `ls --json` — вывести тот же список в формате JSON для скриптов и интеграций.
- `run <target> <cmd>` — запустить Windows-программу через Proton в контексте игры.
  - `--detach` запускает программу в фоне с отключёнными стандартными потоками.
  - `--cwd <path|exe-dir>`, `--env NAME=VALUE` и `--unset-env NAME` позволяют настроить запуск.
- `cmd <target>` — открыть `cmd.exe` в том же префиксе Proton.
- `exec <target> <cmd>` — выполнить нативную Linux-команду с окружением игры.
  - Поддерживает многократное указание параметров `--env NAME=VALUE` и `--unset-env NAME`.
- `env <target>` — вывести сохранённое окружение.
- `info <target> [--json]` — показать подробную информацию об одном активном контексте.
  - В качестве `target` можно указать AppID, `latest` или часть названия игры.
- `doctor [--json] [--fix]` — проверить локальную конфигурацию и сохранённые runtime-контексты; `--fix` удаляет устаревшие сессии и исправляет права доступа.
- `profile` — сохранить, запустить, вывести список или удалить профиль команды.

## 📦 Установка

Скачайте архив для своей системы со страницы [GitHub Releases](https://github.com/hexqnt/protonhax/releases). Для большинства дистрибутивов Linux на базе glibc используйте сборку `x86_64-unknown-linux-gnu`. Сборка `x86_64-unknown-linux-musl` не зависит от системной glibc. Обе готовые сборки требуют процессор с поддержкой x86-64-v3.

Распакуйте архив и установите бинарный файл:

```sh
tar -xzf protonhax-*-x86_64-unknown-linux-*.tar.gz
install -Dm755 protonhax-*-x86_64-unknown-linux-*/bin/protonhax ~/.local/bin/protonhax
```

Или соберите и установите `protonhax` из исходного кода (потребуется [инструментарий Rust](https://rust-lang.org/tools/install/)):

```sh
git clone https://github.com/hexqnt/protonhax.git
cd protonhax
cargo install --path . --locked --root ~/.local
```

Убедитесь, что каталог `~/.local/bin` входит в `PATH`.

## 🕹️ Настройка Steam

Откройте свойства игры в Steam и в поле **Параметры запуска** укажите полный путь к установленному бинарному файлу:

```sh
/home/<user>/.local/bin/protonhax init %command%
```

## 💡 Примеры команд

Вывести список активных игр:

```sh
protonhax ls
# Показать AppID, название, префикс, путь установки и исполняемый файл Proton
protonhax ls -l
```

Пример подробного вывода:

```text
1217060  Gunfire Reborn  started 12m ago
  Prefix: /home/user/.local/share/Steam/steamapps/compatdata/1217060/pfx
  Install: /home/user/.local/share/Steam/steamapps/common/Gunfire Reborn
  Proton: /home/user/.local/share/Steam/steamapps/common/Proton - Experimental/proton
```

Запустить Windows-программу (например, трейнер) в контексте игры с AppID `1217060`:

```sh
protonhax run 1217060 "/home/<user>/Downloads/Gunfire_Reborn_v1.0-v20251025_Plus_8_Trainer.exe"

# Использовать последнюю запущенную активную игру
protonhax run latest "/home/<user>/Downloads/trainer.exe"

# Найти игру по части названия
protonhax run "gunfire" "/home/<user>/Downloads/trainer.exe"

# Запустить трейнер в фоне из его собственного каталога
protonhax run latest --detach --cwd exe-dir "/home/<user>/Downloads/trainer.exe"

# Переопределить переменные окружения для этого запуска
protonhax run latest --env WINEDEBUG=-all --unset-env DXVK_LOG_LEVEL "/home/<user>/Downloads/trainer.exe"
```

Открыть `cmd.exe` в том же префиксе Proton:

```sh
protonhax cmd latest
```

Выполнить нативную Linux-команду с тем же окружением:

```sh
protonhax exec "gunfire" env | sort

# Переопределения окружения доступны и для нативных команд
protonhax exec latest --env MANGOHUD=1 --unset-env WINEDEBUG mangohud --help
```

Просмотреть сохранённое окружение или информацию о контексте:

```sh
protonhax env latest
protonhax info latest
```

Проверить локальную конфигурацию и сохранённые контексты:

```sh
protonhax doctor
# Удалить устаревшие runtime-сессии и исправить права доступа
protonhax doctor --fix
```

## 💾 Профили команд

Профиль сохраняет цель, тип команды, аргументы, рабочий каталог и изменения окружения. ProtonHax атомарно записывает конфигурацию в `$XDG_CONFIG_HOME/protonhax/profiles.json` с правами доступа только для текущего пользователя. Если переменная `XDG_CONFIG_HOME` не задана, используется `~/.config/protonhax/profiles.json`.

```sh
# Сохранить Windows-трейнер
protonhax profile add trainer latest --detach --cwd exe-dir --env WINEDEBUG=-all -- "/home/<user>/Downloads/trainer.exe"

# Запустить сохранённый профиль и добавить аргументы в конец команды
protonhax profile run trainer -- --silent

# Сохранить нативную команду
protonhax profile add game-env latest --kind native -- env

protonhax profile ls
protonhax profile ls --json
protonhax profile remove trainer
```

Полная справка по командам:

```sh
protonhax --help
protonhax run --help
```

## 🧩 Автодополнение команд

Создайте сценарии автодополнения:

```sh
# Bash
protonhax completions bash > ~/.local/share/bash-completion/completions/protonhax

# Zsh
protonhax completions zsh > ~/.zfunc/_protonhax
print -P '%F{yellow}Добавьте в ~/.zshrc: fpath+=(~/.zfunc) && autoload -Uz compinit && compinit%f'

# Fish
protonhax completions fish > ~/.config/fish/completions/protonhax.fish
```

## 🛠️ Отладка и журналирование

Включите отладочный вывод ProtonHax:

```sh
PROTONHAX_DEBUG=1 protonhax ls
```

Runtime-контексты атомарно публикуются, как правило, в `$XDG_RUNTIME_DIR/protonhax`, с доступом только для текущего пользователя. Для каждого запуска игры создаётся отдельная сессия на основе PID. Команды `run` и `exec` игнорируют сессии завершившихся процессов, в том числе оставшиеся после сбоя; `doctor` отмечает их как устаревшие.

Снимки окружения сохраняют Unix-значения без потерь, включая данные не в UTF-8 и переводы строк. В целях безопасности ProtonHax не сохраняет переменные, имена которых содержат `TOKEN`, `PASSWORD`, `SECRET`, `PRIVATE_KEY`, `ACCESS_KEY` или `API_KEY`.

Чтобы перенаправить вывод в файл (это удобно при запуске через Steam):

```sh
/home/<user>/.local/bin/protonhax init %command% &> ~/protonhax.log
```

## ⚠️ Примечания

- Сообщения вида `ERROR: ld.so: object '.../ubuntu12_32/gameoverlayrenderer.so' ... ELFCLASS32` — безвредные предупреждения Steam Overlay, вызванные внедрением 32-битной библиотеки в 64-битный процесс.
- Если игра не запускается, временно включите `PROTONHAX_DEBUG=1` и проверьте журнал.
- Если вы используете Flatpak-версию Steam, поместите бинарный файл ProtonHax в доступный песочнице каталог внутри домашнего каталога и укажите полный путь к нему в **Параметрах запуска**. Команда `protonhax doctor` автоматически определяет Flatpak и выводит рекомендации по настройке, поэтому запускать Steam из терминала не требуется. Чтобы выполнить нативную команду за пределами песочницы, используйте профиль с `flatpak-spawn --host`.
