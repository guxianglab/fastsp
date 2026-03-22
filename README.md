**Language / 中文**: [English](#) | [中文](README_zh.md)

---

# FastSP

**FastSP is an AI-powered voice input method for Windows**: Hold to speak, release to type. It supports both **fast dictation** and **voice commands (Skills)**. Speech recognition is handled by an **online streaming ASR service**, with optional **LLM post-correction** to make spoken text read more like written prose.

- **Streaming Recognition**: Real-time online ASR with partial transcript preview
- **Global Input**: Type directly into any input field (chat, browser, IDE, system dialogs...)
- **Two Modes**: Dictation / Skills
- **Optional AI Scenes**: OpenAI Responses API with structured scene profiles and strict JSON output

## Download & Install (Recommended)

- **Windows Installer (NSIS)**: Download the latest release from [Releases](https://github.com/guxiangjun/fastsp/releases) and install

> For developer builds, see "Development & Build" below.

## Quick Start (30 seconds)

1. **First Launch**: Open Settings and fill in your online ASR credentials
2. **Select Microphone**: Settings -> `Audio Input` -> Choose Input Device (you can test it)
3. **Choose a Trigger** (recommended: start with mouse middle button):
   - **Dictation Mode (types text)**
     - **Mouse middle button hold**: Hold to speak, release to type
     - **Right Alt**: Press once to start recording, press again to stop and type
   - **Skills Mode**
     - **Ctrl + Win hold**: Hold to speak, release to trigger a skill (does not type text)

A **cyan indicator** appears near the mouse while recording; a **red indicator** shows when LLM correction is enabled and processing.

## Why FastSP Feels More Like an "Input Method"

- **More Reliable Typing**: Uses "clipboard write + Ctrl+V" paste, compatible with many Windows native controls (e.g., File Explorer address bar, system search box)
- **Smaller Gap Between Speaking and Writing**: Optional LLM correction focuses on fixing homophones, missing words, punctuation, and word order without changing meaning
- **Commands as Skills**: Upgrades voice from "input" to "action" (open calculator/screenshot/open file explorer...)

## Features Overview

- **Trigger Methods**
  - Mouse middle button hold (Dictation)
  - Right Alt toggle (Dictation)
  - Ctrl + Win hold (Skills)
- **Multi-language**: Auto/Chinese/English/Japanese/Korean/Cantonese
- **Online ASR**
  - Configure App Key, Access Token, and Resource ID in Settings
  - Uses streaming WebSocket recognition with live text preview
  - Supports proxy (HTTP / SOCKS5)
- **LLM Correction (Optional)**
  - OpenAI-compatible API (also works with common third-party/self-hosted gateways)
  - Prompt "Profile" management: create/delete/switch/reset
  - One-click connectivity test
- **Skills (Voice Commands)**
  - Built-in defaults: Email, Calculator, Browser, Notepad, File Manager, Screenshot
  - Each skill can be toggled + custom keywords (comma-separated)
- **History**: Auto-save, copy, clear

## Privacy & Security

- **Speech Recognition Uses Your Configured ASR Provider**: Audio is sent to the online ASR service you configure in Settings.
- **LLM Correction is Optional**: Only when enabled, **recognized text** is sent to your configured LLM endpoint for correction.
- **Controllable & Auditable**: All configuration is in local files; you can disable or clear history at any time.

## Configuration & Data Locations

- **Config File**: `%APPDATA%\com.fastsp\config.json`
- **History File**: `%APPDATA%\com.fastsp\history.json`
Configuration example (subject to actual version):

```json
{
  "trigger_mouse": true,
  "trigger_hold": true,
  "trigger_toggle": true,
  "online_asr_config": {
    "app_key": "",
    "access_key": "",
    "resource_id": "volc.bigasr.sauc.duration"
  },
  "input_device": "",
  "llm_config": {
    "enabled": false,
    "base_url": "https://api.openai.com/v1",
    "api_key": "",
    "model": "gpt-4o-mini",
    "profiles": [
      {
        "id": "default",
        "name": "Default",
        "task_kind": "plain_correction",
        "goal": "Fix obvious ASR errors while keeping the meaning.",
        "tone": "Natural and faithful to the speaker.",
        "format_style": "Return a single paste-ready text block.",
        "preserve_rules": [
          "Preserve names, numbers, and facts.",
          "Do not add information that was not spoken."
        ],
        "glossary": [],
        "examples": [],
        "advanced_instruction": "",
        "expert_mode": false,
        "legacy_imported": false
      }
    ],
    "active_profile_id": "default"
  },
  "proxy": {
    "enabled": false,
    "url": "http://127.0.0.1:7890"
  },
  "skills": [
    {
      "id": "open_calculator",
      "name": "Calculator",
      "keywords": "calculator,calculate,open calculator",
      "enabled": true
    }
  ]
}
```

### LLM Scenes

- FastSP now uses `POST /v1/responses` with strict JSON Schema output.
- Users configure scene fields such as `task_kind`, `goal`, `tone`, `format_style`, `preserve_rules`, `glossary`, and `examples`.
- The app always asks the model for a single structured result and only pastes `final_text`.
- Expert Mode only adds `advanced_instruction`; it does not let users override the hidden developer contract or output schema.
- Older raw prompt profiles are migrated into visible legacy scenes with `legacy_imported: true`.

### Proxy

Proxy applies to both:
- Online ASR requests
- LLM API requests

Examples:
- `http://127.0.0.1:7890`
- `socks5://127.0.0.1:1080`

## Development & Build

### Requirements

- Windows 10/11
- Node.js 18+
- Rust (recommended: latest stable)

### Install Dependencies

```bash
pnpm install
```

### Development Mode

```bash
pnpm tauri dev
```

### Build Installer

```bash
pnpm tauri build
```

Build artifacts are located in `src-tauri/target/release/bundle/` (includes Windows NSIS installer).

### GitHub Actions Build And Release

- `Build Windows Installer` runs automatically on pushes to `main` / `master`, on pull requests, and by manual dispatch.
- `Release Windows Installer` runs automatically when you push a tag like `v1.0.1` and publishes the NSIS installer to GitHub Releases.

Example release flow:

```bash
git tag v1.0.1
git push origin v1.0.1
```

Before using the release workflow, make sure GitHub Actions is enabled for the repository and the default `GITHUB_TOKEN` has permission to create releases.

## Tech Stack (Implementation Overview)

- **Frontend**: React + TypeScript + TailwindCSS + Vite
- **Desktop**: Tauri v2 + Rust
- **ASR**: Online streaming WebSocket ASR
- **Audio Capture**: cpal
- **Global Input Listener**: rdev
- **Typing**: Clipboard + Ctrl+V (enigo)
- **Network**: reqwest (supports HTTP / SOCKS5 proxy)

## FAQ

### 1) Why doesn't Ctrl+Win type text?

This is **Skills Mode**: Ctrl+Win triggers match recognized text to skills and execute actions; by default it **does not type**. For dictation, use mouse middle button or Right Alt.

### 2) Online ASR connection fails?

Check the App Key, Access Token, and Resource ID in Settings first. If you are behind a proxy, configure it in Settings -> Network.

### 3) I'm concerned about privacy

Speech recognition uses your configured online ASR provider. If LLM correction is enabled, recognized **text** is also sent to your configured LLM server. You can disable LLM correction anytime.

## Contributing

Issues and PRs are welcome. Suggestions:
- Include in Issue description: trigger method, target application, reproduction steps, logs/screenshots
- PRs should keep changes focused and avoid introducing unnecessary state and complexity

## License

MIT License

