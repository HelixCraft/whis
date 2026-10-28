<div align="center">
<img src="https://raw.githubusercontent.com/frankdierolf/whis/main/crates/whis-desktop/icons/128x128.png" alt="whis" width="80" height="80" />

<h3>whis</h3>
<p>
  Your voice, piped to clipboard.
  <br />
  <a href="https://whis.ink">Website</a>
  ·
  <a href="https://github.com/frankdierolf/whis/tree/main/crates/whis-desktop">Desktop</a>
  ·
  <a href="https://github.com/frankdierolf/whis/tree/main/crates/whis-mobile">Mobile</a>
  ·
  <a href="https://github.com/frankdierolf/whis/releases">Releases</a>
</p>
</div>

## Why?

- **Built for AI workflows** — speak your prompt, paste to OpenCode, Claude, Codex, ...
- **Cloud or local** — OpenAI, Mistral, Groq, Deepgram, ElevenLabs, or free with local Whisper, Parakeet, Qwen3-ASR, or Moonshine
- **Simple** — record → transcribe → clipboard

## Quick Start

```bash
# Install
cargo install whis-cli

# Setup (interactive wizard)
whis setup

# Run
whis               # Press Enter to stop — text copied!
```

## Screenshot

![whis Demo](https://raw.githubusercontent.com/frankdierolf/whis/main/demo.gif)

## Usage

```bash
# Record once
whis

# Background service mode
whis start         # Start background service
whis toggle        # Toggle recording (bind to your compositor hotkey)

# Post-process with AI (presets define the transformation)
whis --post-process

# Use with terminal AI assistants
claude "$(whis --as ai-prompt)"    # Start session with voice prompt
whis --as ai-prompt --print        # For shell mode (outputs to stdout)

# Presets
whis --as email          # Use preset
whis preset              # List all
whis preset new xyz      # Print template for new preset
whis preset edit xyz     # Edit in $EDITOR

# Transcribe existing audio
whis -f recording.wav

# Help - for you or your helper
whis --help 
```

## Installation

```bash
cargo install whis-cli
```

Or download binaries from [GitHub Releases](https://github.com/frankdierolf/whis/releases).

## Requirements

- API key from [OpenAI](https://platform.openai.com/api-keys), [Mistral](https://console.mistral.ai/api-keys), [Groq](https://console.groq.com/keys), [Deepgram](https://deepgram.com), or [ElevenLabs](https://elevenlabs.io) — or use a local model from the built-in catalog (no API key needed)
- Linux (X11/Wayland), macOS, or Windows

Local models run on the CPU, so no GPU setup is required.

**For hotkey mode** (Linux, pick one):

```bash
# Option 1: Compositor keybinding (no permissions needed)
# GNOME: Settings > Keyboard > Custom Shortcuts → whis toggle
# Sway:  bindsym Ctrl+Alt+w exec whis toggle
# Hyprland: bind = CTRL ALT, W, exec, whis toggle

# Option 2: Direct capture
sudo usermod -aG input $USER && logout
# Then: whis start
```

## Desktop & Mobile

- **[Desktop](https://github.com/frankdierolf/whis/tree/main/crates/whis-desktop)** — GUI with system tray
- **[Mobile](https://github.com/frankdierolf/whis/tree/main/crates/whis-mobile)** — Android app (Alpha)

## Development

```bash
git clone https://github.com/frankdierolf/whis.git
cd whis
just                # List all commands
just install-cli    # Build and install CLI
```

See [CONTRIBUTING.md](https://github.com/frankdierolf/whis/blob/main/CONTRIBUTING.md) for full setup instructions.

## Architecture

![Whis Architecture](https://raw.githubusercontent.com/frankdierolf/whis/main/website/public/architecture.jpg)

## License

MIT
