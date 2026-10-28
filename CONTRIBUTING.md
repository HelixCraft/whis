# Contributing to Whis

Thanks for your interest in contributing! This guide will help you get started.

## Quick Start

Pick what you want to work on - you only need to set up what you need:

| Component | What it is | Start here |
|-----------|------------|------------|
| **Website** | Project website | `just setup-website` |
| **CLI** | Terminal app (Rust) | `just setup-cli` |
| **Desktop** | Desktop app (Tauri + Vue) | `just setup-desktop` |
| **Mobile** | Android app (Tauri + Vue) | `just setup-mobile` |

### Example: Working on the Website

The website is a great place to start - most developers are familiar with web development.

```bash
# Check what you need
just setup-website

# Install dependencies
just deps-website

# Start dev server (hot reload)
just dev-website

# Before committing
just lint-website
just fmt-website
```

### Switching Components

Once you know one, you know them all. Switching from website to desktop:

```bash
# Same pattern, different component
just setup-desktop
just dev-desktop
just lint-desktop
```

## Command Pattern

Every component follows the same pattern:

| Command | Purpose |
|---------|---------|
| `just setup-{component}` | Check/install prerequisites |
| `just deps-{component}` | Fetch dependencies |
| `just dev-{component}` | Run with hot reload |
| `just build-{component}` | Build for release |
| `just lint-{component}` | Check code quality |
| `just fmt-{component}` | Format code |

Replace `{component}` with `cli`, `desktop`, `mobile`, or `website`.

## Prerequisites

Each component has its own requirements. Run `just setup-{component}` to check what you need.

### Using just

We use [just](https://github.com/casey/just) as a task runner. Run `just` to see all available commands.

```bash
cargo install just
just  # see all commands
```

It keeps development and CI nicely aligned across platforms. That said, it's just a convenience - feel free to use `cargo`, `npm`, or whatever you prefer directly.

### CLI

- Rust (latest stable)
- Linux only: ALSA dev libraries (`libasound2-dev`)

### Desktop

- Rust + Node.js 20+
- Linux: WebKit2GTK, AppIndicator, librsvg, patchelf

### Mobile

- Rust + Node.js 20+
- Android Studio with SDK, NDK, and platform-tools
- Rust Android targets (auto-installed by `just setup-mobile`)

### Website

- Node.js 20+

## Project Structure

```
whis/
├── crates/
│   ├── whis-core/      # Core library (providers, audio, config)
│   ├── whis-cli/       # CLI application (package: whis)
│   ├── whis-desktop/   # Tauri desktop app + Vue frontend
│   └── whis-mobile/    # Tauri mobile app (Android)
├── website/            # Project website
└── justfile            # Task automation
```

## Working on Everything

If you need to work across all components (or for CI):

```bash
just setup-all   # Check all prerequisites
just build-all   # Build everything
just check-all   # Run all checks (format + lint)
```

## Making Changes

1. **Fork and clone** the repository
2. **Create a branch** for your changes: `git checkout -b feature/my-feature`
3. **Set up** your component: `just setup-{component}`
4. **Make your changes**
5. **Run checks**: `just lint-{component}` and `just fmt-{component}`
6. **Commit** with a clear message
7. **Open a Pull Request** with a description of your changes

## Code Style

- Run `just fmt-{component}` before committing
- Follow existing patterns in the codebase
- Keep changes focused - one feature/fix per PR
- Add comments for non-obvious logic

## Getting Help

- Open an [issue](https://github.com/frankdierolf/whis/issues) for bugs or questions
- Check existing issues before creating new ones

## Maintainer Notes

I like to express philosophy through priorities:

- CLI > Desktop > Mobile
- Linux > macOS > Windows  
- Android > iOS  
- Cloud > Local

These are just my thoughts for contributions—do what feels right to you. Building for people who love simplicity and openness is kind of the underlying theme.
