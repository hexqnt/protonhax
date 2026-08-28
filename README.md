# protonhax 🚀

Rust‑реализация скрипта [jcnils/protonhax](https://github.com/jcnils/protonhax),
помогающая запускать любые программы в контексте запущенной игры под Steam Proton.

— Сохраняем окружение текущей игры и повторно используем его для ваших команд. ✨

## ✨ Возможности

- `init %command%` — перехват запуска игры от Steam и сохранение контекста (авто).
- `ls` — список текущих игр (AppID и название), для которых сохранён контекст.
  - добавьте `-l` для подробностей (префикс, путь установки, Proton и время старта).
- `ls --json` — тот же список в JSON для скриптов и интеграций.
- `run <target> <cmd>` — запустить Windows‑программу через Proton в контексте игры.
- `cmd <target>` — запустить `cmd.exe` в том же префиксе Proton.
- `exec <target> <cmd>` — запустить нативную Linux‑команду с окружением игры.
  - `target` может быть: `appid`, `latest`, или часть имени игры.
- `doctor` — проверка окружения и сохранённых runtime‑контекстов на ошибки/битые пути.
- `completions <shell>` — генерация автодополнений (bash/zsh/fish/powershell).

## 📦 Установка

Вариант 1 (сборка из исходников):

```sh
git clone https://github.com/hexquant/protonhax.git
cd protonhax
cargo build --release
install -Dm755 target/release/protonhax ~/.local/bin/protonhax
```

Вариант 2 (локальная установка):

```sh
cargo install --path . --locked
```

Убедитесь, что `~/.local/bin` в `PATH`.

Требования: Linux + Steam с Proton; установленный Rust toolchain.

## 🕹️ Использование со Steam

В свойствах игры → Launch Options пропишите полный путь к бинарнику:

```sh
/home/<user>/.local/bin/protonhax init %command%
# или, если у вас так работает:
/home/<user>/.local/bin/protonhax init %COMMAND%
```

## 💡 Примеры CLI

Список активных игр:

```sh
protonhax ls
# или подробный вывод: AppID, название, префикс, путь установки и Proton
protonhax ls -l
# JSON для скриптов
protonhax ls --json
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
protonhax run 1217060 \
  "/home/<user>/Downloads/Gunfire_Reborn_v1.0-v20251025_Plus_8_Trainer.exe"

# Быстрый запуск в контексте последней активной игры
protonhax run latest "/home/<user>/Downloads/trainer.exe"

# Поиск по части имени игры
protonhax run "gunfire" "/home/<user>/Downloads/trainer.exe"
```

Открыть `cmd.exe` в том же префиксе Proton:

```sh
protonhax cmd latest
```

Запустить нативную команду Linux с тем же окружением:

```sh
protonhax exec "gunfire" env | sort
```

Проверить окружение и сохранённые контексты:

```sh
protonhax doctor
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

- Перенаправить вывод в файл (удобно для Steam):

```sh
/home/<user>/.local/bin/protonhax init %command% &> ~/protonhax.log
```

## ⚠️ Примечания

- Сообщения вида
  `ERROR: ld.so: object '.../ubuntu12_32/gameoverlayrenderer.so' ... ELFCLASS32` —
  безвредны и исходят от Steam Overlay (32‑битная библиотека подмешивается в 64‑битный процесс).
- Если игра не стартует — временно включите `PROTONHAX_DEBUG=1` и проверьте лог.
- Для Steam Flatpak запускайте Steam из терминала: `flatpak run com.valvesoftware.Steam` — так легче увидеть вывод.
