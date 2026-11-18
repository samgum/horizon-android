<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/media/readme/title-dark.png">
    <img src="docs/media/readme/title-light.png" alt="Horizon Android" width="320">
  </picture>
</p>

<p align="center">
  <b>Horizon Android (SGMY Edition · 26.3)</b><br>
  A native Rust port of Minecraft: Java Edition 26.3 adapted for Android phones &amp; tablets — zero JVM gameplay runtime, full Java Edition parity, and mobile-first touch controls.
</p>

<p align="center">
  <b>English</b> · <a href="README.zh-CN.md"><b>简体中文</b></a>
</p>

<p align="center">
  <img src="docs/media/readme/hero-lookout.webp" alt="Horizon gameplay screenshot" width="100%">
</p>

---

## Overview

**Horizon Android** brings the [Horizon](https://github.com/horizon-rust/horizon) Rust engine (`26.3`) to Android (`arm64-v8a`, API 28–35) via `NativeActivity`, `winit`, and `wgpu` (Vulkan / OpenGL ES), with comprehensive mobile UX, touch ergonomics, soft-keyboard integration, and display cutout adaptation.

### Key Mobile Highlights

- **Java Edition 26.3 Parity in Native Rust**: Survival, Creative, Hardcore, Redstone, Vanilla & Horizon multiplayer servers, LAN discovery, saves, resource packs, and Horizon mods — running natively on ARM64 without a Java Edition JVM runtime.
- **Mobile-First Gameplay Controls**:
  - **4 Control Schemes** (`Tap` default on fresh installs, `Crosshair`, `Classic` D-Pad + Tap, `Split` D-Pad + Crosshair), plus dynamic floating joystick, sprint latch/rim sprint, double-tap `Jump` creative flight toggle, D-Pad slide-to-move & double-tap sneak, Spyglass zoom sensitivity scaling, and non-overlapping right-hand arc action layout.
  - **Tap vs. Hold Interaction**: Short world taps interact/attack cleanly while press-and-hold mines blocks or uses continuous items (food, bows, shields, tridents), with world-space ray locking so slight finger lift drift never misses your target block.
  - **Hotbar Hold-to-Drop**: Long-press the active hotbar slot with a visual fill bar to toss items directly from the HUD.
- **Bedrock & Classic Container Touch Styles**:
  - **Mobile Bedrock Style (Default)**: Long-press any item stack to scrub a green proportional split bar (`1..N`), swipe scrollable grids without accidental pickups, use the contextual `[>>]` Quick-Move / `[1/2]` Half / `[Q]` Drop side toolbar, and protect against accidental outside-window drops.
  - **Classic Style**: Instant tap pickup/place with `50%` stack split on long-press — switchable anytime in **Options → Controls → Touch Controls…**.
- **Mobile Chat, Command & IME Bridge**:
  - Full Android `InputConnection` bridge supporting CJK Pinyin composition, `IME_ACTION_SEND`, Unicode backspace (`deleteSurroundingTextInCodePoints`), and system `ClipboardManager` copy/paste.
  - Switchable **Top-Docked** (never occluded by the soft keyboard, command suggestions expand downward) or **Follow-Keyboard** (`WindowInsets.Type.ime()`) chat bar, paired with one-tap `[Tab]`, `[↑]`, `[↓]`, `[Paste]`, and `[Send]` quick keys.
- **Display Cutout, Adaptive GUI Scale & Foolproof Launch**:
  - Automatic `DisplayCutout` and gesture-inset safe zones (`shortEdges` immersive sticky fullscreen), mobile-tuned `Auto` GUI scale (`≥ 480×320` logical viewport on `20:9` phones), universal `[X]` screen close button, double-back-to-exit protection, and Bluetooth/USB gamepad auto-hide.
  - Adaptive launcher icon + non-stretching Android 12+ `SplashScreen` + native landscape `BootSplashView` progress overlay that tracks first-launch resource extraction and pre-compiles world shaders before unveiling a stutter-free 60 FPS Main Menu.

---

## Repository Layout

| Path | Description |
| --- | --- |
| `android-apk/` | Android `AndroidManifest.xml`, `HorizonActivity.java` (IME, clipboard, insets, immersive mode, `BootSplashView`), adaptive icons, and themes |
| `crates/horizon-android/` | `cdylib` (`libhorizon_android.so`) `android_main` entry point, JNI bridges, and first-launch `resources.zip` extractor |
| `crates/horizon-client/` | Main client runtime, world/entity simulation integration, startup pipeline, and Android touch/IME/lifecycle handlers |
| `crates/horizon-client-render/` | `wgpu` renderer, HUD, menus, chat UI, touch overlay, layout editor, and Bedrock/Classic container touch state machine |
| `crates/horizon-client-input/` | Window sizing, mobile-aware `Auto` GUI scale calculation, keybindings, and cross-platform/Android clipboard |
| `crates/horizon-prepare/` | Asset & data extractor that builds `horizon-resources.json` and prepared game files from your own copy of *Minecraft: Java Edition 26.3* |

---

## Requirements

1. **Your own legal copy of *Minecraft: Java Edition 26.3***: Horizon ships **no** proprietary Mojang assets. Prepare resources first with `horizon-prepare` and bundle them as `assets/resources.zip` when packaging your APK.
2. **Android Device**: Android 9.0+ (`minSdkVersion 28`, `targetSdkVersion 35`), `arm64-v8a`, with Vulkan 1.1+ or OpenGL ES 3.2 support.
3. **Build Toolchain**:
   - Rust nightly/workspace toolchain (`aarch64-linux-android` target)
   - Android SDK (`platforms/android-35`, `build-tools/35.0.0`) & Android NDK r26+
   - JDK 17+ (`javac`, `d8`, `apksigner` / `zipalign`)

---

## Building & Testing

### 1. Prepare Vanilla 26.3 Resources (Host Machine)

Install *Minecraft: Java Edition 26.3* via the official launcher and run it once, then generate the prepared resource directory:

```bash
cargo run --release -p horizon-prepare -- --prepare
```

Archive the prepared output directory (containing `horizon-resources.json` at its root) into `resources.zip` for bundling under `assets/resources.zip` inside the APK.

### 2. Compile Native Library for Android (`arm64-v8a`)

```bash
rustup target add aarch64-linux-android
cargo build --release --target aarch64-linux-android -p horizon-android
```

The resulting shared library is output to `target/aarch64-linux-android/release/libhorizon_android.so` (packaged into `lib/arm64-v8a/libhorizon_android.so`).

### 3. Package Android Resources & Java Activity

Compile `android-apk/res` with `aapt2`, compile `HorizonActivity.java` against `android-35/android.jar` + `d8` into `classes.dex`, bundle `lib/arm64-v8a/libhorizon_android.so` and `assets/resources.zip`, then align and sign with `zipalign` and `apksigner`.

### 4. Run Desktop Unit & Check Suites

```bash
cargo test -p horizon-client-input --lib
cargo test -p horizon-client-render --lib touch
cargo test -p horizon-client-render --lib container
cargo check -p horizon-client
```

---

## Credits & License

- **Upstream Engine**: Built upon [Horizon](https://github.com/horizon-rust/horizon), founded by [Ludovic Marie](https://lu4ovic.com) and developed by the Horizon Team and [contributors](https://github.com/horizon-rust/horizon/graphs/contributors).
- **License**: Released under the [GNU LGPL v3.0 or later](LICENSE) for original work; code merged prior to the MIT transition remains available under the [MIT License](LICENSES/MIT.txt). Third-party dependencies retain their respective licenses — see [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

<sub>NOT AN OFFICIAL MINECRAFT PRODUCT. NOT APPROVED BY OR ASSOCIATED WITH MOJANG OR MICROSOFT. Minecraft is a trademark of Mojang Synergies AB; its code and assets belong to Mojang and Microsoft under the <a href="https://www.minecraft.net/en-us/eula">Minecraft EULA</a>.</sub>
