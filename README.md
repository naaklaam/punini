# 🦀 Punini

![Rust](https://img.shields.io/badge/Made_with-Rust-orange?style=for-the-badge&logo=rust)
![License](https://img.shields.io/badge/License-MIT-blue?style=for-the-badge)
![Platform](https://img.shields.io/badge/Platform-Linux-lightgrey?style=for-the-badge&logo=linux)

**Punini** is a lightweight, aesthetic, and performance-focused CLI Music Player written in Rust.

It is designed for modern terminal emulators, capable of rendering **High-Resolution Cover Art** directly in the terminal using the Kitty Graphics Protocol, along with synchronized lyrics support.

> *Perfect for ricing enthusiasts on Arch Linux, Hyprland, and Kitty.*

## ✨ Features

* **🎨 Terminal Cover Art:** Renders actual image cover art (not ASCII) using the Kitty Graphics Protocol.
* **🎤 Synchronized Lyrics:** Supports `.lrc` files (karaoke style) and embedded lyrics tags.
* **🎼 Audiophile Ready:** Native support for FLAC, WAV, MP3, OGG, and M4A via `rodio` & `symphonia`.
* **📂 File Browser:** Built-in file navigation to browse your music library.
* **⚡ Blazing Fast:** Built with Rust for minimal memory footprint and high performance.
* **🐧 Linux Native:** Works seamlessly with PipeWire/ALSA.

## 📸 Screenshots

*(Tempatkan screenshot aplikasimu di sini nanti. Contoh: `![Screenshot](assets/screenshot.png)`)*

## 🛠️ Prerequisites

To enjoy the full visual experience (Cover Art), you need a terminal that supports **Kitty Graphics Protocol** or **Sixel**.

Recommended Terminals:
* **Kitty** (Best experience)
* WezTerm
* Ghostty
* Konsole (Recent versions)

Dependencies (Arch Linux):
```bash
sudo pacman -S alsa-lib pkg-config gcc fontconfig
