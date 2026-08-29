# ProtonHax 🚀

[🇺🇸 English](./README.md) · [🇷🇺 Русский](./README.ru.md)

**Run Windows programs in the Proton environment of a running Steam game.**

ProtonHax is a command-line tool for Linux, written in Rust. It launches Windows executables—such as trainers, Cheat Engine, and debugging tools—in the same Proton prefix and environment as a running Steam game. Inspired by the [jcnils/protonhax](https://github.com/jcnils/protonhax) script.

ProtonHax captures a game's Steam Proton environment so you can reuse it to:

- Run `.exe` files in the game's Proton prefix
- Launch Cheat Engine, trainers, and debugging tools through Proton
- Open `cmd.exe` in the running game's Proton environment
- Execute native Linux commands with the game's environment variables
- Find running games by Steam AppID or name
- Diagnose broken or stale Proton contexts

## ✨ Features

- `init %command%` — intercept a game launched by Steam and save its context.
- `ls` — list active games (AppID and name) with a saved context.
  - Add `-l` to show the prefix, installation path, Proton executable, and start time.
- `ls --json` — output the same list as JSON for scripts and integrations.
- `run <target> <cmd>` — run a Windows program through Proton in a game's context.
  - `--detach` runs the program in the background with its standard streams disconnected.
  - `--cwd <path|exe-dir>`, `--env NAME=VALUE`, and `--unset-env NAME` customize the launch.
- `cmd <target>` — open `cmd.exe` in the same Proton prefix.
- `exec <target> <cmd>` — run a native Linux command with the game's environment.
  - Supports repeated `--env NAME=VALUE` and `--unset-env NAME` options.
- `env <target>` — print the saved environment.
- `info <target> [--json]` — show detailed information about one active context.
  - `target` can be an AppID, `latest`, or part of a game name.
- `doctor [--json] [--fix]` — check the local setup and saved runtime contexts; `--fix` removes stale sessions and repairs permissions.
- `profile` — save, run, list, or remove command profiles.

## 📦 Installation

Download the archive for your system from [GitHub Releases](https://github.com/hexqnt/protonhax/releases). Use the `x86_64-unknown-linux-gnu` build on most glibc-based Linux distributions. The `x86_64-unknown-linux-musl` build does not depend on the system's glibc. Both release builds require an x86-64-v3-compatible CPU.

Extract the archive and install the binary:

```sh
tar -xzf protonhax-*-x86_64-unknown-linux-*.tar.gz
install -Dm755 protonhax-*-x86_64-unknown-linux-*/bin/protonhax ~/.local/bin/protonhax
```

Alternatively, build and install `protonhax` from source (requires the [Rust toolchain](https://rust-lang.org/tools/install/)):

```sh
git clone https://github.com/hexqnt/protonhax.git
cd protonhax
cargo install --path . --locked --root ~/.local
```

Make sure `~/.local/bin` is in your `PATH`.

## 🕹️ Steam setup

Open the game's properties in Steam and enter the full path to the installed binary under **Launch Options**:

```sh
/home/<user>/.local/bin/protonhax init %command%
```

## 💡 CLI examples

List active games:

```sh
protonhax ls
# Show AppID, name, prefix, installation path, and Proton executable
protonhax ls -l
```

Example of detailed output:

```text
1217060  Gunfire Reborn  started 12m ago
  Prefix: /home/user/.local/share/Steam/steamapps/compatdata/1217060/pfx
  Install: /home/user/.local/share/Steam/steamapps/common/Gunfire Reborn
  Proton: /home/user/.local/share/Steam/steamapps/common/Proton - Experimental/proton
```

Run a Windows program (for example, a trainer) in the context of the game with AppID `1217060`:

```sh
protonhax run 1217060 "/home/<user>/Downloads/Gunfire_Reborn_v1.0-v20251025_Plus_8_Trainer.exe"

# Use the most recently started active game
protonhax run latest "/home/<user>/Downloads/trainer.exe"

# Find a game by part of its name
protonhax run "gunfire" "/home/<user>/Downloads/trainer.exe"

# Run a trainer in the background from its own directory
protonhax run latest --detach --cwd exe-dir "/home/<user>/Downloads/trainer.exe"

# Override environment variables for this launch
protonhax run latest --env WINEDEBUG=-all --unset-env DXVK_LOG_LEVEL "/home/<user>/Downloads/trainer.exe"
```

Open `cmd.exe` in the same Proton prefix:

```sh
protonhax cmd latest
```

Run a native Linux command with the same environment:

```sh
protonhax exec "gunfire" env | sort

# Environment overrides are also available for native commands
protonhax exec latest --env MANGOHUD=1 --unset-env WINEDEBUG mangohud --help
```

Inspect a saved environment or context:

```sh
protonhax env latest
protonhax info latest
```

Check the local setup and saved contexts:

```sh
protonhax doctor
# Remove stale runtime sessions and repair permissions
protonhax doctor --fix
```

## 💾 Command profiles

A profile stores the target, command type, arguments, working directory, and environment changes. ProtonHax writes the configuration atomically to `$XDG_CONFIG_HOME/protonhax/profiles.json` with permissions restricted to the current user. If `XDG_CONFIG_HOME` is not set, it uses `~/.config/protonhax/profiles.json`.

```sh
# Save a Windows trainer
protonhax profile add trainer latest --detach --cwd exe-dir --env WINEDEBUG=-all -- "/home/<user>/Downloads/trainer.exe"

# Run the saved profile and append extra arguments
protonhax profile run trainer -- --silent

# Save a native command
protonhax profile add game-env latest --kind native -- env

protonhax profile ls
protonhax profile ls --json
protonhax profile remove trainer
```

For complete command-line help:

```sh
protonhax --help
protonhax run --help
```

## 🧩 Shell completions

Generate completion scripts:

```sh
# Bash
protonhax completions bash > ~/.local/share/bash-completion/completions/protonhax

# Zsh
protonhax completions zsh > ~/.zfunc/_protonhax
print -P '%F{yellow}Add to ~/.zshrc: fpath+=(~/.zfunc) && autoload -Uz compinit && compinit%f'

# Fish
protonhax completions fish > ~/.config/fish/completions/protonhax.fish
```

## 🛠️ Debugging and logging

Enable ProtonHax debug output:

```sh
PROTONHAX_DEBUG=1 protonhax ls
```

Runtime contexts are published atomically, normally under `$XDG_RUNTIME_DIR/protonhax`, with access restricted to the current user. Each game launch gets a separate PID-based session. Commands such as `run` and `exec` ignore sessions whose processes have exited, including those left behind by a crash; `doctor` reports them as stale.

Environment snapshots preserve Unix values losslessly, including non-UTF-8 data and line breaks. For security, ProtonHax does not save variables whose names contain `TOKEN`, `PASSWORD`, `SECRET`, `PRIVATE_KEY`, `ACCESS_KEY`, or `API_KEY`.

To redirect output to a file (useful when launching through Steam):

```sh
/home/<user>/.local/bin/protonhax init %command% &> ~/protonhax.log
```

## ⚠️ Notes

- Messages such as `ERROR: ld.so: object '.../ubuntu12_32/gameoverlayrenderer.so' ... ELFCLASS32` are harmless Steam Overlay warnings caused by a 32-bit library being injected into a 64-bit process.
- If the game does not start, temporarily enable `PROTONHAX_DEBUG=1` and inspect the log.
- For the Flatpak version of Steam, place the ProtonHax binary somewhere in your home directory that the sandbox can access, then use its full path in **Launch Options**. `protonhax doctor` detects Flatpak automatically and provides setup guidance, so you do not need to launch Steam from a terminal. To run a native command outside the sandbox, use a profile with `flatpak-spawn --host`.
