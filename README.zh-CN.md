<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/media/readme/title-dark.png">
    <img src="docs/media/readme/title-light.png" alt="Horizon Android" width="320">
  </picture>
</p>

<p align="center">
  <b>Horizon Android（SGMY Edition · 26.3）</b><br>
  基于纯 Rust 从零重写的 Minecraft: Java Edition 26.3 安卓原生移植版 —— 无 JVM 游戏运行时、完整 Java 版特性对齐、专为触屏与全面屏深度定制的移动交互体验。
</p>

<p align="center">
  <a href="README.md"><b>English</b></a> · <b>简体中文</b>
</p>

<p align="center">
  <img src="docs/media/readme/hero-lookout.webp" alt="Horizon 游戏实机画面" width="100%">
</p>

---

## 项目简介

**Horizon Android** 将 [Horizon](https://github.com/horizon-rust/horizon) 纯 Rust 客户端引擎（对应 *Minecraft: Java Edition 26.3*）原生移植至 Android 平台（`arm64-v8a`，API 28–35）。底层通过 `NativeActivity` + `winit` + `wgpu`（Vulkan / OpenGL ES）直接驱动渲染与音频，并针对手机与平板的触控手势、背包交互、软键盘输入法、挖孔刘海安全区以及启动加载流程进行了系统级重做。

### 核心移动端特性

- **原生 Rust 驱动的 Java 版 26.3 体验**：完整支持生存、创造、极限模式、红石逻辑、原版与 Horizon 多人联机服务器、局域网发现、原版存档直读、资源包与 Horizon 原生模组系统，无需在手机端运行庞大耗电的 Java 虚拟机。
- **符合直觉的触屏游戏操控**：
  - **4 套触控方案**（首次安装默认启用 **摇杆 + 点按交互 `Tap`**，同时支持 **摇杆 + 十字准星 `Crosshair`**、**方向键 + 点按 `Classic`**、**方向键 + 十字准星 `Split`**）。
  - **完整移动手感细节**：支持动态浮动摇杆、冲刺锁定/推满外圈自动冲刺、创造模式双击跳跃起飞/降落、十字方向键中心滑动平移与双击潜行、望远镜开镜视角减速、以及互不重叠的右手弧形按键布局与可视化布局编辑器。
  - **点按与长按精准分离 + 3D 射线锁定**：世界内轻点触发单次交互/攻击，长按连续挖掘方块或使用弓弩/食物/盾牌/三叉戟；手指抬起瞬间锁定世界 3D 射线，彻底消除触屏抬手微移导致的放歪方块问题。
  - **快捷栏长按丢弃**：长按底部快捷栏当前选中槽位显示进度条，蓄满即可快速丢出物品。
- **双模式背包与容器触控（基岩版风格 + 经典风格可切换）**：
  - **手机基岩版风格（默认 `Bedrock`）**：长按任意堆叠物品滑动绿色比例分堆条（`1..N` 精准拆分），在创造物品栏等可滚动网格支持滑动翻页防误拿，侧边自动浮现 `[>>]` 快速转移、`[1/2]` 对半拆分、`[Q]` 丢弃快捷工具栏，并内置容器外误触防丢保护。
  - **经典桌面风格（`Classic`）**：点按拿起/放下、长按自动对半拆分，可在 **设置 → 控制 → 触屏控制…** 中随时一键切换。
- **移动端聊天栏、指令补全与中文输入法桥接**：
  - 原生 Android `InputConnection` 桥接，完整支持中文拼音候选上屏、键盘回车/发送键（`IME_ACTION_SEND`）、Emoji/多字节字符退格以及系统剪贴板复制粘贴。
  - 支持 **顶部停靠（默认，绝不被软键盘遮挡，指令补全向下展开）** 与 **跟随软键盘高度（`WindowInsets.Type.ime()` 动态上移）** 两种模式切换，并配备 `[Tab]` 补全、`[↑]`/`[↓]` 历史消息、`[Paste]` 粘贴与 `[Send]` 发送触控按钮。
- **双模式账号中心（免登录离线测试 + 微软正版登录共存）**：
  - 默认启用 **离线测试模式（`Player`）**，启动后**零弹窗、免登录直达主菜单**，支持随时自定义离线玩家名进行单机与离线服快速测试。
  - 点击主菜单左上角账号胶囊即可打开原生账号中心，通过 **微软 OAuth 2.0 设备码**（自动复制验证码并一键拉起手机浏览器）完成 Xbox Live → XSTS → Minecraft 正版验证，采用 **Android Keystore 硬件级 `AES-256-GCM` 加密**保存令牌并支持自动续期、正版服进服校验与运行时无感热切换（支持配置自定义 Azure `HORIZON_MSA_CLIENT_ID` 与导入调试 Session）。
- **全面屏安全区、自适应 GUI 缩放与防呆开屏系统**：
  - 自动读取 `DisplayCutout` 挖孔与系统边缘返回手势安全区，沉浸式全屏常亮（游戏内防熄屏，退回菜单允许休眠），针对 `20:9` 细长手机屏幕优化 `Auto` GUI 缩放（保证至少 `480×320` 逻辑像素空间）、全局二级界面右上角 `[X]` 关闭按钮、返回键双击防误退提示以及蓝牙/USB 手柄接入自动隐藏触控层。
  - **防拉伸开屏 + 零卡顿主界面切换**：采用 Android 自适应图标 + Android 12+ 标准矢量 `SplashScreen` + 原生横屏防呆加载层 `BootSplashView`，实时展示首次启动 `resources.zip` 解压进度与 3D 渲染管线/着色器预编译状态，拦截加载期误触，直至首帧 60 FPS 主界面渲染就绪后平滑淡出。

---

## 目录结构

| 路径 | 说明 |
| --- | --- |
| `android-apk/` | Android 清单 `AndroidManifest.xml`、`HorizonActivity.java`（输入法、剪贴板、窗口安全区、沉浸全屏、`BootSplashView` 防呆开屏层）、自适应图标与主题样式 |
| `crates/horizon-android/` | 安卓 `cdylib`（`libhorizon_android.so`）入口 `android_main`、JNI 桥接与首次启动 `resources.zip` 带进度解压逻辑 |
| `crates/horizon-client/` | 客户端主运行时、单机/联机世界循环、启动管线以及 Android 触控/输入法/生命周期集成 |
| `crates/horizon-client-render/` | `wgpu` 渲染器、HUD、设置与主菜单 UI、移动端聊天栏、触控覆盖层与基岩版/经典容器触控状态机 |
| `crates/horizon-client-input/` | 窗口尺寸管理、移动端自适应 `Auto` GUI 缩放算法、按键绑定与跨平台/Android 剪贴板 |
| `crates/horizon-prepare/` | 资源预处理工具，从正版 *Minecraft: Java Edition 26.3* 提取并生成运行所需的 `horizon-resources.json` 资源目录 |

---

## 环境与前置要求

1. **正版 *Minecraft: Java Edition 26.3* 游戏本体**：本仓库**不包含**任何受版权保护的 Mojang 游戏资产。打包 APK 前需先在电脑端通过正版启动器下载并运行一次 26.3，使用 `horizon-prepare` 生成资源目录后压缩为 `assets/resources.zip`。
2. **Android 运行设备**：Android 9.0 及以上（`minSdkVersion 28`，`targetSdkVersion 35`），`arm64-v8a` 64 位架构，支持 Vulkan 1.1+ 或 OpenGL ES 3.2。
3. **构建工具链**：
   - Rust 工具链（安装 `aarch64-linux-android` 编译目标）
   - Android SDK（`platforms/android-35`、`build-tools/35.0.0`）与 Android NDK r26+
   - JDK 17+（`javac`、`d8`、`zipalign`、`apksigner`）

---

## 构建与测试指南

### 1. 提取并准备 26.3 游戏资源（宿主机）

在已安装并启动过 *Minecraft: Java Edition 26.3* 的电脑上执行：

```bash
cargo run --release -p horizon-prepare -- --prepare
```

将生成的资源目录内容（确保根目录直接包含 `horizon-resources.json`）打包为 `resources.zip`，放入 APK 构建目录的 `assets/resources.zip`。应用首次安装启动时会自动展示进度并将其解压至内部存储。

### 2. 交叉编译 Android 原生动态库（`arm64-v8a`）

```bash
rustup target add aarch64-linux-android
cargo build --release --target aarch64-linux-android -p horizon-android
```

产物位于 `target/aarch64-linux-android/release/libhorizon_android.so`（打包至 APK 内 `lib/arm64-v8a/libhorizon_android.so`）。

### 3. 编译 Android 资源与签名打包

使用 `aapt2` 编译并链接 `android-apk/res` 与 `AndroidManifest.xml`，使用 `javac` + `d8` 将 `HorizonActivity.java` 编译为 `classes.dex`，合并 `lib/arm64-v8a/libhorizon_android.so` 与 `assets/resources.zip` 后通过 `zipalign` 与 `apksigner` 对齐签名即可安装运行。

### 4. 运行本地单元测试与静态检查

```bash
cargo test -p horizon-client-input --lib
cargo test -p horizon-client-render --lib touch
cargo test -p horizon-client-render --lib container
cargo check -p horizon-client
```

---

## 致谢与开源协议

- **上游引擎**：本项目基于 [Horizon](https://github.com/horizon-rust/horizon) 构建（由 [Ludovic Marie](https://lu4ovic.com) 创立，Horizon 团队与[社区贡献者](https://github.com/horizon-rust/horizon/graphs/contributors)共同开发）。
- **开源许可**：项目原创代码遵循 [GNU LGPL v3.0 及后续版本](LICENSE) 发布；自 MIT 迁移前合入的代码仍可在 [MIT 许可证](LICENSES/MIT.txt) 下使用。第三方依赖保留各自开源协议，详见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。

<sub>本项目并非 MINECRAFT 官方产品，未获 MOJANG 或 MICROSOFT 批准，亦与 MOJANG 或 MICROSOFT 无任何关联。Minecraft 是 Mojang Synergies AB 的商标，相关原始代码与美术音频资产归 Mojang 与 Microsoft 所有并受 <a href="https://www.minecraft.net/en-us/eula">Minecraft 最终用户许可协议 (EULA)</a> 约束。</sub>
