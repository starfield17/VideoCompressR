# Video Compressor：Rust + Tauri 2 最终重写架构手册

> Grilled Edition / Final Architecture Candidate
>
> 目标：用 Rust + Tauri 2 完整重写现有 Video Compressor，复刻 CLI、GUI、功能与 UX 行为，支持 Windows、macOS、Linux，并使项目可以主要由 AI Harness 完成编码、测试、修复和发布。
>
> 本手册不是“最大而全”的架构，而是经过反向质疑后得到的最小充分架构。任何新增抽象都必须先证明它解决了当前存在的行为、测试或平台边界问题。

---

## 0. 最终结论

最终采用：

**Rust Modular Monolith + DDD-lite Core + Application Runtime + Thin Adapters + Contract/Golden-Master Migration**。

核心结构只有两层共享 Rust 库：

1. `vc-core`：纯模型、规则、计划生成、状态转换；
2. `vc-runtime`：FFmpeg/FFprobe、队列编排、配置、预设、文件系统和应用服务；

再加两个入口：

3. `video-compressor`：独立 CLI；
4. `video-compressor-desktop`：Tauri 2 桌面应用。

测试共享代码放入：

5. `vc-testkit`：fake ffmpeg、fixture、临时目录和 contract test 工具，仅作为开发依赖。

依赖方向：

```text
vc-core
   ↑
vc-runtime
   ↑          ↑
CLI       Tauri Desktop

vc-testkit 仅作为测试依赖指向 vc-core / vc-runtime
Frontend 只能通过 Tauri IPC 接触 Desktop Adapter
```

### 最重要的决定

- CLI 与 GUI 是两个独立二进制，不把 CLI 塞进 Tauri GUI 启动模式。
- GUI 不调用 CLI 子进程；二者链接同一套 Rust 库。
- DDD 只用于真正存在规则的区域，不建设教科书式 Repository/Aggregate 仪式。
- 队列的合法状态转换属于 `vc-core`；异步 worker、锁、取消令牌属于 `vc-runtime`。
- 不创建独立 `contracts` crate；Tauri DTO 留在 Desktop Adapter，CLI 输出留在 CLI Adapter。
- 不创建通用 Event Bus；使用 `tokio::sync::watch` 和 `broadcast`。
- 不引入数据库、事件溯源、插件系统、远程服务、队列断点恢复。
- 第一版不重新设计 UX；先冻结并复刻旧版本行为。
- FFmpeg 只能由 Rust 运行时启动，前端不能获得 shell/spawn 权限。
- 所有迁移以旧 Python 版本的 Golden Master 和行为矩阵为准。

---

## 1. 必须先明确的假设与边界

### 1.1 “UX 1:1”不是跨渲染引擎逐像素相同

旧程序使用 Qt Widgets，新程序使用系统 WebView。字体 hinting、滚动条、原生控件尺寸、DPI 和抗锯齿无法保证逐像素一致。

本项目中的 1:1 定义为：

#### 必须完全一致

- 功能入口；
- 字段、默认值和字段顺序；
- CLI 参数、帮助、stdout/stderr 和退出码；
- 计划、命令和编码结果；
- 按钮启用条件；
- 队列状态和调度语义；
- 暂停、停止、重试、删除、重排；
- 窗口关闭流程；
- 预设和配置语义；
- 中英文文本含义；
- 发布目标。

#### 视觉上高度接近

- 信息架构；
- 控件位置和比例；
- 字号、颜色、边距；
- 表格列和上下文菜单；
- 主窗口、Queue、Activity Log、Settings、Preset、Preview 窗口。

#### 可接受差异

- 平台字体渲染；
- 系统滚动条；
- 1–3 px 以内的布局差异；
- 标题栏和系统窗口装饰。

若“Qt 原生控件逐像素一致”是硬性要求，则 Tauri 2 不是正确技术选型，应改用 Qt/Slint 类原生 GUI。本文继续采用 Tauri 2，默认接受上述工程化定义。

### 1.2 不新增当前项目不存在的产品能力

第一版明确不做：

- 队列跨重启恢复；
- 数据库；
- 云同步；
- 插件系统；
- 远程控制；
- 自动下载任意第三方 FFmpeg；
- 自动更新器；
- 新编码格式；
- GUI 改版；
- CLI 新的 JSON API，除非迁移期间明确需要。

这些能力以后可以通过 ADR 单独加入，但不能污染重写主线。

### 1.3 人类不写代码，不等于人类不做产品决策

AI Harness 可以独立完成：

- 分析；
- 写测试；
- 编码；
- 修复；
- 重构；
- 构建；
- 发布自动化。

但以下情况必须形成显式 Decision Request，由人类只做选择，不参与实现：

- 旧行为互相矛盾；
- 跨平台无法同时复刻；
- 许可证影响发行方式；
- UX 1:1 与可访问性、安全性冲突；
- 需要改变公开 CLI/配置 schema。

低层实现歧义不得频繁打断人类。AI 应优先采用“最小变化、兼容旧行为、最少抽象”的保守答案，并记录假设。

---

## 2. Grill：对上一版架构逐条质疑后的答案

### Q1：为什么不是完整 DDD？

**答案：** 项目复杂度来自外部进程、硬件能力、队列状态和跨平台发布，而不是复杂商业语言。完整 DDD 会增加 Aggregate、Repository、Domain Service、Mapper 和 DTO 仪式。

**最终处理：** 只在 `vc-core` 中建模值、规则和状态转换，称为 DDD-lite。

### Q2：为什么不保留 `domain/application/infrastructure/contracts` 四个库？

**答案：** 对 1.1 万行旧项目而言，四个共享库会导致大量跨 crate 修改和映射；AI 容易为了完成一个小功能同时改五处。

**最终处理：** 收敛为 `vc-core` 和 `vc-runtime` 两个共享库。应用编排与基础设施在 `vc-runtime` 内按模块隔离，而不是按 crate 仪式隔离。

### Q3：为什么仍然需要两个共享库，而不是一个大 crate？

**答案：** 必须有一道编译期边界保证核心规则不依赖 Tokio、Tauri、进程和文件系统。单 crate 只能靠约定，AI 更容易越界。

**最终处理：** `vc-core` 是强边界；`vc-runtime` 可以使用异步和系统能力。

### Q4：为什么删除 `vc-contracts`？

**答案：** CLI 和 GUI 的外部协议不同。强行共享 DTO 会把前端展示需求反向污染领域模型，也会产生大量无价值映射。

**最终处理：** Desktop DTO 位于 `apps/desktop/src-tauri/contracts`；CLI 输出模型位于 `apps/cli/output`。二者都从 runtime/core 类型映射。

### Q5：每个 I/O 是否都需要 Port/trait？

**答案：** 不需要。只有存在第二实现或测试替代需求的边界才需要 trait。临时目录足以测试真实 JSON Store，没有必要为每个配置文件创建 Repository trait。

**最终处理：** 默认使用具体类型；只有进程执行/流式运行这类真正需要 fake 的边界允许窄 trait。

### Q6：Queue 是 Domain Aggregate 吗？

**答案：** 队列中的合法状态变化是领域规则，但异步 worker、锁、进程句柄、取消和订阅不是领域对象。

**最终处理：**

- `vc-core::queue`：纯 `QueueState`、`QueueItem` 和 transition/reducer；
- `vc-runtime::queue::QueueSupervisor`：异步调度、并发槽、进程取消、事件发布。

### Q7：是否需要 Event Bus？

**答案：** 不需要通用总线。当前只有“最新队列快照”和“活动日志流”两种语义。

**最终处理：**

- `watch<Arc<QueueSnapshot>>`：保留最新状态；
- `broadcast<ActivityEvent>`：传输日志和离散事件。

### Q8：是否需要持久化队列？

**答案：** 旧程序没有可靠的跨重启队列恢复契约。新增会引入恢复、幂等和输出冲突问题。

**最终处理：** 第一版队列只存在内存中。仅持久化设置、预设、能力缓存和窗口状态。

### Q9：GUI 是否应通过调用 CLI 来确保逻辑共享？

**答案：** 不应。这样会重复序列化状态、增加进程生命周期问题，队列和多窗口同步也会更复杂。

**最终处理：** CLI 和 GUI 都直接链接 `vc-runtime`。

### Q10：前端是否需要 Redux/Zustand 等全局状态？

**答案：** 后端已经是业务状态唯一来源。大型前端 store 容易形成第二套队列状态机。

**最终处理：** React `useReducer`/Context 管理表单和视图状态；队列快照直接来自 IPC Channel。暂不引入全局状态库。

### Q11：FFmpeg 是否由 JavaScript sidecar API 启动？

**答案：** 不应。WebView 获得 shell 权限会扩大攻击面，也会绕过 Rust 规则。

**最终处理：** FFmpeg/FFprobe 始终由 Rust `vc-runtime` 启动。Tauri 只负责定位已打包工具并把路径传给 runtime。

### Q12：是否继续解析 stderr 进度？

**答案：** 不作为主协议。FFmpeg 提供 `-progress` 的机器可读 `key=value` 流。

**最终处理：** 使用 `-progress pipe:1 -nostats`；stderr 只保留诊断。

### Q13：是否需要把所有数字包装成 newtype？

**答案：** 不需要。过度 newtype 会增加转换噪声。

**最终处理：** 只包装容易混淆或必须验证的单位：`BitrateBps`、`Percent`、`Seconds`、`CompressionRatio`。宽高、pass index 等使用普通整数。

### Q14：是否需要任意 worker 数量的通用线程池？

**答案：** 旧行为是按显式 backend 分配 worker，且有 backend 特定限制。

**最终处理：** 一个选中的 backend 对应一个 slot；不要抽象成通用 CPU 线程池。

### Q15：是否应在第一版启用 updater？

**答案：** 不应。更新器依赖签名、发布稳定性和 rollback 设计。

**最终处理：** 完成签名和发布 contract 后再单独加入。

### Q16：GUI 自动化是否只能覆盖 Windows/Linux？

**答案：** 当前 Tauri 文档推荐 WebdriverIO Tauri service，并支持 Windows、Linux 和 macOS 的嵌入式 WebDriver；可以用于三平台 E2E。

**最终处理：** 快速测试使用 browser mode；打包 E2E 使用 WebdriverIO Tauri service。

### Q17：是否应该设置强制单文件行数上限？

**答案：** 固定 LOC 门槛容易让 AI 机械拆文件，制造碎片。

**最终处理：** 不以行数作为合并门禁；以单一职责、变更范围、依赖方向和测试覆盖验收。只对异常大文件发出 review warning。

### Q18：是否需要从第一天就支持 Thin 和 Full 两种发行？

**答案：** 架构需要支持，但实现顺序不应并行展开。

**最终处理：** 先完成 Thin；Full 在核心稳定后加入，并单独处理 FFmpeg 构建来源和许可证。

### Q19：是否允许 Agent 顺便清理旧代码？

**答案：** 不允许。迁移期间最危险的是无法追踪行为变化来源。

**最终处理：** 每一处修改必须能追溯到当前任务的 Goal 或测试；无关问题只记录，不顺手修。

### Q20：什么才是 AI Coding 友好的核心？

**答案：** 不是层数多，而是任务边界可验证。

**最终处理：** 每个任务必须给出兼容契约、允许路径、禁止路径、测试、命令和 non-goals；Harness 以测试结果而不是“看起来完成”结束。

---

## 3. 现有项目事实与迁移对象

当前项目约有：

- 11,130 行 Python；
- 134 个测试；
- CLI：`plan`、`encode`、`preview`、`preset`；
- GUI：主窗口、Queue、Activity Log、Settings、Preset Manager、Preview Result；
- 编码：HEVC、AV1；
- backend：Auto、CPU、NVENC、QSV、AMF、VideoToolbox；
- 预览采样；
- 串行和多 backend 并行；
- 暂停于当前任务完成后、停止、重试、重排；
- 能力探测和一帧真实 smoke test；
- 配置、预设、能力缓存、中英文；
- Windows/macOS/Linux，x86_64/ARM64 原生发布目标。

因此迁移单位不是 Python 文件，而是行为契约：

```text
输入 + 配置 + 平台能力
          ↓
扫描结果
          ↓
MediaInfo
          ↓
Encoder selection
          ↓
EncodePlan
          ↓
FFmpeg argv
          ↓
进度 / 日志 / 结果
          ↓
CLI 或 GUI 展示
```

旧 Python 版本在迁移期是 Reference Implementation。

---

## 4. 最终 Workspace

```text
video-compressor/
├── Cargo.toml
├── Cargo.lock
├── rust-toolchain.toml
├── rustfmt.toml
├── clippy.toml
├── deny.toml
├── package.json
├── pnpm-lock.yaml
├── AGENTS.md
├── ARCHITECTURE.md
├── FEATURE_PARITY.md
├── CLI_CONTRACT.md
├── UX_CONTRACT.md
├── RELEASE_CONTRACT.md
│
├── crates/
│   ├── vc-core/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── model/
│   │       │   ├── codec.rs
│   │       │   ├── media.rs
│   │       │   ├── settings.rs
│   │       │   ├── encoder.rs
│   │       │   ├── plan.rs
│   │       │   ├── preview.rs
│   │       │   └── units.rs
│   │       ├── planning/
│   │       │   ├── bitrate.rs
│   │       │   ├── encoder_selection.rs
│   │       │   ├── output_path.rs
│   │       │   ├── validation.rs
│   │       │   └── planner.rs
│   │       ├── queue/
│   │       │   ├── model.rs
│   │       │   ├── transition.rs
│   │       │   └── metrics.rs
│   │       └── error.rs
│   │
│   ├── vc-runtime/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── application.rs
│   │       ├── bootstrap.rs
│   │       ├── planning.rs
│   │       ├── preview.rs
│   │       ├── ffmpeg/
│   │       │   ├── discovery.rs
│   │       │   ├── ffprobe.rs
│   │       │   ├── capabilities.rs
│   │       │   ├── command.rs
│   │       │   ├── progress.rs
│   │       │   ├── process.rs
│   │       │   └── cancel.rs
│   │       ├── queue/
│   │       │   ├── supervisor.rs
│   │       │   ├── scheduler.rs
│   │       │   ├── worker.rs
│   │       │   └── events.rs
│   │       ├── storage/
│   │       │   ├── app_config.rs
│   │       │   ├── presets.rs
│   │       │   ├── capability_cache.rs
│   │       │   └── atomic_json.rs
│   │       ├── platform/
│   │       │   ├── paths.rs
│   │       │   ├── process_flags.rs
│   │       │   └── tool_layout.rs
│   │       ├── subtitles.rs
│   │       ├── scanner.rs
│   │       ├── activity.rs
│   │       └── error.rs
│   │
│   └── vc-testkit/
│       ├── Cargo.toml
│       └── src/
│           ├── fake_tool.rs
│           ├── fixtures.rs
│           ├── golden.rs
│           └── temp_app.rs
│
├── apps/
│   ├── cli/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── main.rs
│   │       ├── args.rs
│   │       ├── commands/
│   │       ├── output.rs
│   │       ├── i18n.rs
│   │       └── exit_code.rs
│   │
│   └── desktop/
│       ├── package.json
│       ├── vite.config.ts
│       ├── src/
│       │   ├── main.tsx
│       │   ├── api/
│       │   │   ├── client.ts
│       │   │   ├── channels.ts
│       │   │   └── generated.ts
│       │   ├── model/
│       │   ├── features/
│       │   │   ├── source/
│       │   │   ├── encoding-options/
│       │   │   ├── planning/
│       │   │   ├── queue/
│       │   │   ├── preview/
│       │   │   ├── presets/
│       │   │   ├── settings/
│       │   │   └── activity-log/
│       │   ├── windows/
│       │   ├── components/
│       │   ├── i18n/
│       │   └── styles/
│       └── src-tauri/
│           ├── Cargo.toml
│           ├── build.rs
│           ├── tauri.conf.json
│           ├── capabilities/
│           ├── resources/
│           └── src/
│               ├── lib.rs
│               ├── main.rs
│               ├── state.rs
│               ├── bootstrap.rs
│               ├── contracts/
│               ├── commands/
│               ├── subscriptions.rs
│               └── error.rs
│
├── fixtures/
│   ├── legacy/
│   │   ├── cli/
│   │   ├── plans/
│   │   ├── commands/
│   │   ├── presets/
│   │   ├── capabilities/
│   │   └── screenshots/
│   ├── media/
│   └── fake-tools/
│
├── scripts/
│   ├── capture_legacy.py
│   ├── check_architecture.py
│   ├── stage_ffmpeg.py
│   ├── generate_bindings.sh
│   └── package_smoke.py
│
├── e2e/
│   ├── browser/
│   └── desktop/
│
└── .github/workflows/
    ├── ci.yml
    ├── parity.yml
    ├── e2e.yml
    └── release.yml
```

---

## 5. 编译期边界

### 5.1 `vc-core` 允许依赖

只允许轻量、纯数据依赖，例如：

- `serde`；
- `thiserror`；
- 必要时的 `uuid` 类型，但 ID 生成在 runtime；
- 标准库。

### 5.2 `vc-core` 禁止依赖

- `tokio`；
- `tauri`；
- `clap`；
- `tracing`；
- `std::process` / `tokio::process`；
- Tauri DTO；
- React/TypeScript 概念；
- 具体配置目录；
- FFmpeg 可执行文件发现。

### 5.3 `vc-runtime` 职责

`vc-runtime` 可以使用 Tokio、tracing、文件系统和进程，但不能出现：

- Tauri command；
- CLI parser；
- UI 文案布局；
- React DTO；
- 平台窗口操作。

### 5.4 Adapter 规则

- CLI handler 只做参数解析、调用 runtime、格式化输出和退出码映射；
- Tauri command 只做 DTO 映射、权限入口和调用 runtime；
- 前端只能通过 `src/api/client.ts` 使用 `invoke`；
- 前端不拼 FFmpeg 参数；
- 前端不直接读写预设/config；
- 前端不推断任务是否成功。

---

## 6. `vc-core`：真正需要 DDD-lite 的部分

### 6.1 稳定模型

```rust
pub enum Codec {
    Hevc,
    Av1,
}

pub enum EncoderBackend {
    Auto,
    Cpu,
    Nvenc,
    Qsv,
    Amf,
    VideoToolbox,
}

pub enum DecodeAcceleration {
    Software,
    VideoToolbox,
}

pub enum AudioMode {
    Copy,
    Aac,
}

pub enum ContainerFormat {
    Mkv,
    Mp4,
}
```

`EncodeSettings` 保留旧项目字段和默认值：

```rust
pub struct EncodeSettings {
    pub codec: Codec,
    pub backend: EncoderBackend,
    pub decode_acceleration: DecodeAcceleration,
    pub parallel_enabled: bool,
    pub parallel_backends: Vec<EncoderBackend>,
    pub ratio: Option<CompressionRatio>,
    pub min_video_bitrate: BitrateBps,
    pub max_video_bitrate: Option<BitrateBps>,
    pub container: ContainerFormat,
    pub audio_mode: AudioMode,
    pub audio_bitrate: String,
    pub copy_subtitles: bool,
    pub copy_external_subtitles: bool,
    pub two_pass: bool,
    pub encoder_preset: Option<String>,
    pub pixel_format: String,
    pub maxrate_factor: f64,
    pub bufsize_factor: f64,
    pub overwrite: bool,
    pub recursive: bool,
    pub dry_run: bool,
}
```

第一版不得重新命名公开 JSON 字段，除非 migration 层同时支持旧名字。

### 6.2 只使用必要 newtype

```rust
pub struct BitrateBps(u64);
pub struct Seconds(f64);
pub struct Percent(f32);
pub struct CompressionRatio(f64);
```

这些类型在构造时校验范围。不要为 width、height、worker index 等所有整数建立包装类型。

### 6.3 Planner 是纯规则

I/O 不进入 Planner：

```rust
pub struct PlanningInput {
    pub source: MediaInfo,
    pub output_path: PathBuf,
    pub settings: EncodeSettings,
    pub capabilities: CapabilitySnapshot,
}

pub fn plan_item(input: PlanningInput) -> Result<EncodePlanItem, PlanningError>;
```

Runtime 负责扫描、调用 ffprobe、读取能力缓存；Core 负责：

- backend/codec 兼容；
- Auto backend 优先级；
- 默认 preset；
- 码率计算；
- two-pass 限制；
- VideoToolbox 限制；
- parallel 限制；
- 输出路径；
- warning/skip 原因。

### 6.4 Plan 是不可变执行快照

```rust
pub struct EncodePlanItem {
    pub source_path: PathBuf,
    pub output_path: PathBuf,
    pub media: MediaInfo,
    pub encoder: EncoderSelection,
    pub settings: EncodeSettings,
    pub target_video_bitrate: BitrateBps,
    pub warnings: Vec<PlanWarning>,
    pub skip_reason: Option<SkipReason>,
}
```

加入队列后，任务保存完整 PlanItem 快照。用户之后修改 GUI 表单不应改变已入队任务。

### 6.5 Queue 只保存纯状态

```rust
pub enum QueueItemState {
    Draft,
    Queued,
    Running,
    Done,
    Failed,
    Skipped,
    Cancelled,
}

pub enum QueueRunState {
    Idle,
    Running,
    PauseRequested,
    Paused,
    Cancelling,
}
```

不保留没有实际转换的 `Paused` item 状态；“队列暂停”是 QueueRunState，未执行 item 仍为 `Queued`。若 legacy fixture 证明 UI 确实需要 item paused，再加入。

合法转换由一个集中 reducer 完成：

```rust
pub enum QueueCommand {
    Enqueue(Vec<QueueItem>),
    StartItem { item_id: QueueItemId, run_id: RunId },
    ReportProgress { item_id: QueueItemId, progress: ItemProgress },
    Finish { item_id: QueueItemId, result: ItemResult },
    Fail { item_id: QueueItemId, error: JobError },
    Cancel { item_id: QueueItemId, reason: String },
    Retry { item_ids: Vec<QueueItemId> },
    Remove { item_ids: Vec<QueueItemId> },
    Reorder { ordered_ids: Vec<QueueItemId> },
    ClearCompleted,
}

pub fn apply(state: &mut QueueState, command: QueueCommand)
    -> Result<(), QueueError>;
```

禁止 Runtime 或 UI 直接写 `item.state = ...`。

---

## 7. `vc-runtime`：应用编排与系统边界

### 7.1 Application 是 composition root，不是 God Object

```rust
pub struct Application {
    pub planning: PlanningService,
    pub preview: PreviewService,
    pub queue: Arc<QueueSupervisor>,
    pub presets: PresetService,
    pub settings: SettingsService,
    pub capabilities: CapabilityService,
    pub activity: ActivityHub,
}
```

这些服务对应现有产品功能，而不是为了架构形式随意拆分。

### 7.2 启动流程

```text
resolve app paths
    ↓
load/migrate app config
    ↓
discover ffmpeg + ffprobe
    ↓
validate tool identity
    ↓
load or refresh capability cache
    ↓
construct services
    ↓
publish BootstrapSnapshot
```

Bootstrap 失败必须返回结构化诊断，不能 panic。

### 7.3 不创建无价值 Repository trait

以下使用具体实现：

- `JsonPresetStore`；
- `JsonAppConfigStore`；
- `JsonCapabilityCache`；
- `AtomicJsonWriter`。

测试通过 tempfile 和真实文件系统完成。

只有当某边界确实需要两种运行实现或 fake 时才引入 trait。首个允许的 trait 是窄进程边界，例如：

```rust
#[async_trait]
pub trait ToolRunner: Send + Sync {
    async fn run_capture(&self, request: ToolRequest) -> Result<ToolOutput, ToolError>;
    async fn spawn_streaming(
        &self,
        request: ToolRequest,
        sink: ProgressSink,
        cancel: CancellationToken,
    ) -> Result<ToolExit, ToolError>;
}
```

它只抽象外部工具进程，不抽象整个文件系统或整个 FFmpeg 业务。

---

## 8. FFmpeg/FFprobe 设计

### 8.1 工具发现顺序必须成为 contract

建议保持旧行为并显式测试：

1. 用户明确路径；
2. 应用打包目录中的完整 `ffmpeg` + `ffprobe` 对；
3. 应用配置路径；
4. `PATH`；
5. 平台常见位置，如 Homebrew/Scoop；
6. 失败并输出诊断。

不允许发现一个来源的 ffmpeg 和另一个不匹配版本的 ffprobe，除非旧行为明确允许且有测试。

### 8.2 Typed command，禁止 shell 字符串

```rust
pub struct ToolRequest {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub cwd: Option<PathBuf>,
    pub stdin: StdinMode,
    pub stdout: StdoutMode,
    pub stderr: StderrMode,
}
```

Command Builder 输出 `Vec<OsString>`，不经过 shell：

```rust
pub fn render_encode_commands(item: &EncodePlanItem) -> Vec<ToolRequest>;
```

这解决：

- Windows 空格路径；
- 非 UTF-8 路径；
- shell 注入；
- 参数顺序漂移；
- CLI/GUI 逻辑分叉。

### 8.3 进度协议

每个 encode command 增加：

```text
-hide_banner
-nostats
-progress pipe:1
-stats_period 0.5
```

stdout 解析：

```text
frame=...
fps=...
bitrate=...
out_time_us=...
speed=...
progress=continue|end
```

parser 必须：

- 忽略未知 key；
- 接受缺失字段；
- 对非法数字返回 warning 而不是 panic；
- 以 `progress=` 结束一个 batch；
- 处理分块读取而非假设一 read 等于一行。

stderr 单独进入滚动日志和错误分类。

### 8.4 取消协议

不使用 shell，直接持有 child handle。

取消顺序：

1. 设置 `CancellationToken`；
2. 尝试向 FFmpeg stdin 写入 `q\n`，允许正常收尾；
3. 等待短 grace period；
4. 若仍运行，调用平台硬终止；
5. 必须 `wait()` 回收 child；
6. 清理 two-pass passlog 和未完成临时文件；
7. 通过 `run_id` 防止迟到进度污染新任务。

所有分支都必须有测试：正常结束、取消、强杀、spawn 失败、读流失败。

### 8.5 Capability detection

能力探测分为：

- `ffmpeg -version` identity；
- encoder 列表；
- hwaccel 列表；
- encoder help/preset；
- 一帧真实 smoke test。

缓存 fingerprint 至少包括：

```text
schema_version
ffmpeg canonical path
file size
modified timestamp
version digest
OS
architecture
capability algorithm version
```

GPU/驱动摘要只有在稳定可得时加入，不能为了“完美缓存”依赖脆弱平台命令。

### 8.6 并行调度

保持现有语义：

- `parallel_backends` 去重且排除 Auto；
- 每个 backend 一个 worker slot；
- 结果按输入顺序呈现；
- parallel 拒绝 two-pass；
- parallel 拒绝手工 preset；
- 任一 worker 发生系统级失败时停止领取新任务并取消其它 active worker；
- 单个文件编码失败是否终止全队列，以 legacy contract 为准，不静默改变。

---

## 9. QueueSupervisor

### 9.1 内部结构

```rust
pub struct QueueSupervisor {
    state: tokio::sync::Mutex<QueueState>,
    run: tokio::sync::Mutex<Option<ActiveRun>>,
    snapshots: tokio::sync::watch::Sender<Arc<QueueSnapshot>>,
    activity: tokio::sync::broadcast::Sender<ActivityEvent>,
}

struct ActiveRun {
    run_id: RunId,
    cancel: CancellationToken,
    join: JoinHandle<RunOutcome>,
}
```

强制规则：

- 不得持有 `state` 锁跨 `.await`；
- 每次读取后生成执行快照，再释放锁；
- worker 回报事件必须包含 `run_id` 和 `item_id`；
- 非当前 run 的迟到事件直接丢弃；
- 所有状态变化通过 `vc-core::queue::apply`。

### 9.2 Start

```text
validate QueueRunState
    ↓
选择 Queued items
    ↓
创建 run_id + CancellationToken
    ↓
状态变为 Running
    ↓
启动 scheduler task
    ↓
发布 snapshot
```

### 9.3 Pause after current

```text
Running -> PauseRequested
停止向空闲 slot 分配新 item
active item 正常完成
所有 active slot 空闲
PauseRequested -> Paused
```

未开始 item 保持 `Queued`，再次 Start 可继续。

### 9.4 Stop

```text
Running/PauseRequested -> Cancelling
取消所有 active token
active item -> Cancelled
未开始 item 默认保持 Queued
Cancelling -> Idle
```

这里建议保持未开始任务为 `Queued`，便于用户再次启动。最终以旧 GUI 实测 contract 为准。

### 9.5 Retry/Remove/Reorder

- Running item 不允许 retry/remove/reorder；
- Failed/Cancelled 可 retry 回 Queued；
- Done/Skipped 是否允许删除按旧行为；
- Busy 时只允许不会影响 active execution snapshot 的操作；
- 重排只能改变未开始 item 的顺序。

---

## 10. CLI

### 10.1 独立二进制

```text
video-compressor plan ...
video-compressor encode ...
video-compressor preview ...
video-compressor preset ...
```

迁移阶段可提供兼容 shim：

```text
video-compressor --cli plan ...
```

但正式 CLI 不依赖 Tauri，也不启动 WebView。

### 10.2 CLI adapter 只负责

- Clap parser；
- 旧参数名称和默认值；
- preset merge precedence；
- 调用 runtime；
- human-readable 输出；
- i18n；
- exit code。

不得在 CLI 中重新实现：

- backend 选择；
- 码率规则；
- command builder；
- parallel scheduler；
- preset validation。

### 10.3 退出码 contract

在 Phase 0 捕获旧行为后固定。建议分类：

```text
0 success
2 CLI usage / validation
3 tool discovery or capability failure
4 planning failure
5 encode/preview failure
130 user cancellation on Unix-compatible CLI behavior
```

若旧版本不同，优先兼容旧版本；不要擅自采用建议值。

### 10.4 Ctrl+C

CLI 安装 signal handler，把 Ctrl+C 转成同一个 `CancellationToken`；必须等待 child 被回收后再退出，避免遗留 ffmpeg。

---

## 11. Tauri 2 Desktop Adapter

### 11.1 Tauri 不是业务层

Tauri state：

```rust
pub struct DesktopState {
    pub app: Arc<vc_runtime::Application>,
}
```

Command 示例：

```rust
#[tauri::command]
async fn plan_encode(
    state: State<'_, DesktopState>,
    request: PlanRequestDto,
) -> Result<PlanResponseDto, ApiErrorDto> {
    let input = request.try_into()?;
    let plan = state.app.planning.plan(input).await?;
    Ok(plan.into())
}
```

Command 中禁止出现 FFmpeg argv 拼接、文件扫描、队列状态修改细节。

### 11.2 IPC 类型

Tauri DTO 使用：

- `serde`；
- `ts-rs` 生成 TypeScript 类型；
- `camelCase` JSON；
- tagged enum；
- 稳定错误 code。

```rust
#[derive(Serialize, TS)]
#[serde(tag = "type", content = "data", rename_all = "camelCase")]
pub enum QueueStreamMessage {
    Snapshot(QueueSnapshotDto),
    Activity(ActivityEventDto),
    Closed { reason: String },
}
```

生成的 `generated.ts` 由 CI 校验，不允许手写镜像类型。

### 11.3 Command 与 Channel

普通 request/response 使用 commands：

- bootstrap；
- plan；
- preview；
- queue mutations；
- settings；
- presets。

有序流使用 Tauri Channel：

- queue snapshot；
- activity log；
- capability scan progress；
- preview/encode progress。

每个窗口建立自己的 subscription。不要用高频全局 event 广播完整队列。

### 11.4 Capabilities

按窗口最小授权：

```text
main window:
  plan, preview, queue mutate, settings, preset, dialog

queue window:
  queue read/mutate, subscription

activity window:
  activity read/export, subscription
```

不同 capability 不得无意覆盖同一窗口并合并为更大权限。

前端不授予：

- shell execute/spawn；
- 任意文件系统访问；
- 任意路径打开；
- FFmpeg sidecar 直接调用。

文件选择使用 dialog，返回路径后仍由 Rust 验证。

### 11.5 多窗口

所有窗口共享同一个 Rust `Application`。

窗口只保存视图状态，不保存业务真相。Queue 窗口关闭后不会停止 QueueSupervisor；重新打开时先获取最新 snapshot，再订阅后续变化。

---

## 12. React/TypeScript 前端

### 12.1 技术栈

- React；
- TypeScript strict；
- Vite；
- CSS Modules + CSS variables；
- Vitest；
- Testing Library；
- WebdriverIO Tauri service；
- pnpm，固定版本。

第一版不使用大型 UI Design System，以免破坏 1:1 复刻。

### 12.2 状态分类

#### 后端权威状态

- Queue items；
- run state；
- progress；
- encoder capability；
- preset 内容；
- encode result；
- tool status。

#### 前端本地状态

- 当前 Tab；
- 选中行；
- 弹窗开关；
- 表单尚未提交的草稿；
- 表格列宽和临时排序；
- hover/focus。

### 12.3 不引入第二套状态机

前端不能这样做：

```ts
item.status = "done";
```

前端只能请求：

```ts
await api.retryQueueItems(ids);
```

然后等待后端 snapshot。

### 12.4 API 集中

只有：

```text
apps/desktop/src/api/client.ts
apps/desktop/src/api/channels.ts
```

可以出现 `invoke` 和 Channel 注册。其它组件只能调用 typed client。

### 12.5 UX 复刻方法

Phase 0 为每个窗口建立：

- 基准分辨率；
- 最小尺寸；
- 默认尺寸；
- 控件树；
- Tab 顺序；
- enabled/disabled 矩阵；
- 错误弹窗场景；
- screenshots；
- 表格列行为；
- 小屏滚动合同。

禁止边迁移边“优化”布局。

---

## 13. 配置、预设和数据目录

### 13.1 文件类型

```text
AppConfig
Preset
CapabilityCache
WindowState
```

每个文件有 `schemaVersion`。

### 13.2 写入协议

```text
serialize
  ↓
write temp file in same directory
  ↓
flush
  ↓
atomic rename/replace
  ↓
保留原文件直到替换成功
```

损坏配置不能导致应用无法启动：

- 记录诊断；
- 将坏文件改名为 `.broken-<timestamp>`；
- 使用默认配置；
- 不静默覆盖原文件。

### 13.3 Python 兼容迁移

Rust 第一版必须读取旧 preset 字段，包括：

- 缺失的新字段；
- 空字符串 preset -> `None`；
- 旧 parallel 字段默认；
- decode acceleration 默认；
- default preset；
- language；
- tool paths。

迁移成功后写新 schema，但保留旧文件备份。

### 13.4 数据目录

使用各平台标准用户应用目录：

```text
config/
presets/
cache/
logs/
workdir/
previews/
temp/
```

CLI 默认与 GUI 使用同一目录，但允许命令行覆盖 config/workdir/tool paths。

---

## 14. FFmpeg 分发

### 14.1 Thin 首先

第一阶段正式构建为 Thin：

- 不内置 FFmpeg；
- 支持用户路径、应用目录和 PATH 发现；
- 缺失时提供完整诊断；
- 先稳定核心行为和发布链。

### 14.2 Full 后加入

Full 版本使用每目标预先准备的 `ffmpeg`/`ffprobe` 完整配对。

为保持 `vc-runtime` 与 Tauri 解耦，打包脚本把工具放入一个可预测的 executable resource layout；Desktop bootstrap 解析为具体 PathBuf，之后仍由 runtime 的同一 ToolRunner 启动。禁止从 JavaScript 调用 shell 插件。

每个 binary 生成 manifest：

```json
{
  "target": "aarch64-apple-darwin",
  "ffmpegVersion": "...",
  "sourceRevision": "...",
  "configureFlags": ["..."],
  "licenseMode": "LGPL-or-GPL",
  "sha256": "..."
}
```

CI 必须验证：

- target/architecture；
- ffmpeg/ffprobe 配对；
- executable bit；
- `-version` 可运行；
- build manifest；
- 第三方许可证。

---

## 15. 多平台发布

目标保持六个原生 target：

| OS | Arch | Desktop | CLI |
|---|---|---|---|
| Windows | x86_64 | installer + portable | zip |
| Windows | ARM64 | installer + portable | zip |
| macOS | x86_64 | app + DMG | tar.gz |
| macOS | ARM64 | app + DMG | tar.gz |
| Linux | x86_64 | AppImage + tar.gz | tar.gz |
| Linux | ARM64 | AppImage + tar.gz | tar.gz |

原则：

- 尽量在对应原生 runner 构建；
- macOS 同时测试 Intel target 和 Apple Silicon target；
- Windows ARM64 使用原生/受控 ARM runner；
- Linux 在明确的最老支持基线上构建；
- AppImage 测试 X11 和 Wayland 启动；
- packaged smoke 不等同于 `cargo test`。

正式 Release gate：

- macOS 签名和 notarization；
- Windows Authenticode；
- checksums；
- SBOM；
- licenses；
- CLI `--help`；
- GUI 启动；
- tool discovery；
- 真实短视频 CPU encode；
- cancel smoke；
- preset read/write。

签名尚未建立时，可以发布明确标记的 unsigned preview，但不得称为 production release。

---

## 16. 测试架构

### 16.1 Legacy Golden Master

Phase 0 从 Python 版本捕获：

```text
CLI help/stdout/stderr/exit code
preset JSON
app config JSON
capability cache
MediaInfo fixtures
EncodePlan fixtures
FFmpeg argv fixtures
preview fixtures
queue transition scenarios
GUI screenshots
window geometry
release artifact names/layout
```

Golden fixture 必须包含输入和生成环境，不允许只有期望输出。

### 16.2 Core unit tests

覆盖：

- default settings；
- bitrate policy；
- Auto backend；
- codec/backend compatibility；
- VideoToolbox；
- two-pass；
- parallel validation；
- preset compatibility；
- output path；
- preview window；
- queue transitions；
- metrics/ETA；
- stale run event rejection。

### 16.3 Runtime contract tests

使用 fake executable，而不是 mock 函数堆：

- ffprobe 输出 fixture JSON；
- ffmpeg encoder/hwaccel 输出；
- progress 分块；
- invalid UTF-8；
- exit non-zero；
- hang；
- graceful cancel；
- forced kill；
- Windows 空格路径；
- two-pass cleanup。

这样测试的是 argv、stdio、取消和进程生命周期的真实边界。

### 16.4 Real FFmpeg smoke

在 CI/Release runner：

- 生成短测试视频；
- probe；
- CPU HEVC；
- CPU AV1 在合理超时内可选；
- preview；
- subtitles；
- external subtitles；
- two-pass；
- cancel；
- output verification。

硬件 backend 由专用机器定期测试，普通 hosted runner 不假设有 GPU。

### 16.5 CLI contract tests

- help snapshots；
- parser；
- bool positive/negative flags；
- preset precedence；
- invalid preset；
- auto backend + manual preset；
- stdout/stderr；
- cancellation；
- packaged executable。

### 16.6 Frontend tests

快速层：

- Vitest + Testing Library；
- mock typed API；
- 表单联动；
- backend filter；
- table；
- dialogs；
- i18n；
- browser-mode visual regression。

桌面层：

- WebdriverIO Tauri service；
- Windows/Linux/macOS；
- real IPC；
- multiwindow；
- close while busy；
- queue subscription；
- screenshot baseline；
- packaged app smoke。

---

## 17. AI Harness Coding Protocol

### 17.1 顶层 `AGENTS.md`

必须包含以下固定原则：

```text
1. State assumptions before coding.
2. Prefer the simplest compatibility-preserving change.
3. Do not add unrequested flexibility.
4. Touch only files allowed by the task.
5. Do not clean unrelated code.
6. Define tests before declaring success.
7. Do not change public contracts silently.
8. Stop and emit a Decision Request when contracts are ambiguous.
9. Every changed line must trace to Goal or Acceptance.
10. No agent may claim completion without running required commands.
```

### 17.2 每个任务格式

```yaml
task_id: VC-PLAN-HEVC-003
goal: >
  Implement the pure HEVC bitrate planning rule and match legacy fixtures.

compatibility_contract:
  fixtures:
    - fixtures/legacy/plans/hevc_cpu/*.json
  public_behavior:
    - same target bitrate
    - same warnings
    - same skip reasons

assumptions:
  - no new codec
  - no CLI changes

allowed_paths:
  - crates/vc-core/src/planning/bitrate.rs
  - crates/vc-core/tests/bitrate.rs

forbidden_paths:
  - crates/vc-runtime/**
  - apps/**
  - Cargo.lock

non_goals:
  - refactor encoder selection
  - rename existing types
  - add configurable policies

acceptance:
  - cargo fmt --check
  - cargo clippy -p vc-core --all-targets -- -D warnings
  - cargo test -p vc-core bitrate
```

### 17.3 Agent 自问清单

编码前必须回答并写入 task report：

1. 当前行为由哪个 fixture/test 证明？
2. 能否用更小改动完成？
3. 是否真的需要新抽象或依赖？
4. 哪些文件必须改，哪些明确不改？
5. 失败时什么测试会先红？
6. 是否改变 CLI/IPC/schema/UX？
7. 如何验证跨平台差异？

### 17.4 并行规则

- 同一文件同时只能有一个 writer；
- 大范围重构不得并行；
- Agent 可并行读取和分析，但 edit ownership 必须互斥；
- 跨层 vertical slice 由 supervisor 分顺序任务；
- `Cargo.lock`、generated bindings、release manifest 由单一集成任务更新；
- Agent 不得在共享工作区执行破坏性 reset。

### 17.5 完成循环

```text
read task + contracts
    ↓
state assumptions
    ↓
add/fix test
    ↓
minimal implementation
    ↓
run focused tests
    ↓
run package checks
    ↓
inspect diff against allowed paths
    ↓
report evidence
```

“代码已写完”不是完成；只有 Acceptance 全绿才是完成。

### 17.6 CI 门禁

每个 PR：

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo deny check
pnpm lint
pnpm typecheck
pnpm test
python scripts/check_architecture.py
```

架构检查至少拒绝：

- `vc-core` 依赖 tokio/tauri/clap/process；
- Tauri command 中出现 `Command::new` 或 FFmpeg argv；
- CLI 中出现 backend 选择规则；
- frontend 在 `api/` 外出现 `invoke(`；
- frontend shell/spawn 权限；
- 手写 generated DTO；
- production 路径新增 `unwrap()`/`expect()`；
- lock 跨 await；
- contract 变化但 fixture/schema 未更新；
- 修改超出 allowed paths。

---

## 18. 迁移阶段

### Phase 0：冻结 Reference Implementation

产物：

- `FEATURE_PARITY.md`；
- CLI snapshots；
- plan/argv golden fixtures；
- preset/config/cache fixtures；
- GUI screenshot/interaction matrix；
- queue scenarios；
- release artifact contract。

验收：核心行为不是“记忆描述”，而是可运行 fixture。

### Phase 1：Workspace + Core skeleton

实现：

- workspace；
- `vc-core`/`vc-runtime`/CLI/Desktop skeleton；
- architecture check；
- CI；
- error model；
- generated TS pipeline。

不实现转码。

### Phase 2：Core planning parity

顺序迁移：

1. enums/default settings；
2. preset compatibility model；
3. bitrate policy；
4. output path；
5. backend selection；
6. VideoToolbox；
7. parallel/two-pass validation；
8. preview window。

验收：纯 fixture 全部通过。

### Phase 3：FFprobe + capability + plan CLI

实现：

- discovery；
- ffprobe；
- capability detection/cache；
- `plan` CLI。

验收：Rust plan 与 Python golden 等价。

### Phase 4：单文件 encode vertical slice

仅完成：

- CPU HEVC；
- typed command；
- machine progress；
- log；
- cancel；
- output result。

不要同时实现所有 backend。

### Phase 5：补齐编码矩阵

按 fixture 顺序：

- CPU AV1；
- NVENC；
- QSV；
- AMF；
- VideoToolbox；
- audio/subtitle；
- external subtitle；
- two-pass；
- preview。

### Phase 6：CLI parity complete

完成：

- plan；
- encode；
- preview；
- preset；
- i18n；
- exit codes；
- package smoke。

此时后端核心才允许被 GUI 使用。

### Phase 7：QueueSupervisor

实现：

- serial；
- backend slots；
- pause after current；
- stop；
- retry；
- remove；
- reorder；
- metrics；
- watch/broadcast；
- stale run protection。

### Phase 8：Tauri shell + IPC

实现：

- Application state；
- commands；
- channels；
- capabilities；
- DTO generation；
- multiwindow skeleton。

验收：GUI 可启动并读 bootstrap/queue snapshot，但暂不追求视觉。

### Phase 9：GUI 1:1 vertical slices

按功能区域逐一完成：

1. Source + output；
2. encode option tabs；
3. plan summary；
4. queue table；
5. progress/status；
6. preview；
7. presets；
8. settings；
9. activity log；
10. Queue window；
11. close while busy；
12. i18n；
13. responsive/minimum geometry；
14. visual regression。

### Phase 10：数据迁移

- 导入旧 config/preset；
- migration backup；
- capability cache invalidation；
- Windows/macOS/Linux 路径测试。

### Phase 11：Thin 六平台发布

- native matrix；
- installers/archives；
- package smoke；
- signing；
- SBOM；
- checksums。

### Phase 12：Full FFmpeg package

- curated builds；
- manifest；
- licenses；
- executable resource staging；
- six target smoke。

### Phase 13：切换 Rust，归档 Python

只有在所有 parity gate 通过后：

- Rust 成为默认；
- Python 不参与运行和发布；
- Python 源码归档到最后 tag；
- fixtures 和 migration tools 保留。

---

## 19. 第一批可以直接交给 Harness 的任务

建议起始 Wave：

### Wave A：只读分析与冻结

- A1：枚举 134 个测试并映射到 feature matrix；
- A2：捕获所有 CLI help/exit/output；
- A3：捕获 preset/config/cache schema；
- A4：运行旧 GUI 并生成窗口/交互截图矩阵；
- A5：捕获 FFmpeg argv golden fixtures。

这些任务可以并行，因为不修改同一实现文件。

### Wave B：骨架，顺序执行

- B1：创建 workspace 和 `vc-core`；
- B2：创建 `vc-runtime`；
- B3：创建 CLI skeleton；
- B4：创建 Tauri skeleton；
- B5：加入 architecture check 和 CI。

### Wave C：Core 规则，按文件所有权并行

- C1：settings/enums；
- C2：bitrate policy；
- C3：output path；
- C4：queue transitions；
- C5：preview window。

Supervisor 最后做一次集成任务更新 exports 和 Cargo.lock。

---

## 20. 明确拒绝的架构

### 拒绝：一个 `src-tauri/main.rs` 包含所有逻辑

原因：不可测试、CLI 无法共享、AI diff 冲突严重。

### 拒绝：GUI 启动 CLI 子进程

原因：重复状态、错误处理和进程层，队列同步困难。

### 拒绝：完整 Clean Architecture crate 金字塔

原因：对当前规模过度，映射代码多于业务代码。

### 拒绝：每个存储文件一个 Repository interface

原因：只有一个 JSON 实现，tempdir 足以测试。

### 拒绝：通用事件总线

原因：实际只有 snapshot 和 activity 两种流。

### 拒绝：数据库和队列恢复

原因：旧行为无 contract，会扩大项目范围。

### 拒绝：前端 shell 权限

原因：绕过 Rust 边界，扩大安全面。

### 拒绝：第一版顺便重做 UX

原因：无法区分迁移 bug 和产品改动。

### 拒绝：一次性让 Agent “重写整个项目”

原因：不可验收、不可定位、不可恢复。

---

## 21. Definition of Done

完整重写必须同时满足：

### 运行时

- Python 不参与运行；
- GUI/CLI 共享 `vc-core` 和 `vc-runtime`；
- CLI 是独立二进制；
- FFmpeg 不经过 shell；
- progress 使用机器协议；
- cancel 无遗留进程；
- queue 无非法状态转换；
- stale progress 不污染新 run。

### 兼容性

- CLI contract 通过；
- plan/argv golden 通过；
- preset/config migration 通过；
- 所有 backend 规则通过；
- parallel/pause/cancel/retry/reorder 通过；
- GUI feature matrix 完整；
- i18n 完整；
- UX screenshot 在阈值内。

### 发布

- Windows x86_64/ARM64；
- macOS x86_64/ARM64；
- Linux x86_64/ARM64；
- CLI archives；
- Desktop packages；
- package smoke；
- signatures；
- SBOM/licenses/checksums；
- Full 构建具有 FFmpeg provenance。

### AI Harness

- 所有任务有 Goal/Contract/Allowed Paths/Acceptance；
- 单 writer 文件所有权；
- CI 阻止依赖越界；
- 无静默 contract 修改；
- 每个完成声明都附带实际命令结果；
- 人类不需要编写或手动修补代码。

---

## 22. 最终判断

该项目最适合的不是“最纯粹的 DDD”，而是：

> **用一个纯 Rust Core 固定规则，用一个 Runtime 封装所有不确定的系统世界，再让 CLI 和 Tauri 只做翻译。**

这套架构的优点不是结构图好看，而是：

- AI 每次只需要理解有限模块；
- Rust 编译器能阻止最危险的依赖越界；
- 旧 Python 行为可以逐条 Golden Master 替换；
- CLI 先完成，降低 GUI 迁移风险；
- 进程、取消、并行和多窗口只有一个权威实现；
- 不为未来想象的需求提前付出复杂度；
- 任一失败都能定位到 Core rule、Runtime boundary、CLI adapter 或 Desktop adapter。

在当前需求下，这是我最终推荐的架构基线。后续新增内容应以 ADR 方式说明：解决什么已确认问题、为什么现有结构无法满足、增加了哪些测试和复杂度。

---

## 参考资料

- Tauri 2：Calling Rust from the Frontend / Commands
- Tauri 2：Calling the Frontend from Rust / Channels
- Tauri 2：Permissions、Capabilities、Runtime Authority
- Tauri 2：WebDriver / WebdriverIO Tauri service
- Tauri 2：GitHub Actions distribution pipeline
- FFmpeg：`-progress`、`-stats_period`、stdin interaction
