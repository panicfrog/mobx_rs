# `mobx_rs` FFI 方案设计（中文版）

## 1. 背景与目标

- **核心目标**：把现有的 MobX 风格运行时通过 C ABI 暴露给 Swift、Kotlin/NDK、Flutter/Dart、React Native 等宿主，让业务状态继续保留在宿主语言，Rust 仅负责依赖追踪与调度。
- **不做的事情**：UI 绑定、store 代码生成、proc-macro。宿主自己维护数据结构，Rust 不承载业务逻辑。
- **约束条件**
  - 保持当前 Rust API 对纯 Rust 使用者可用。
  - 提供稳定的 C ABI（静态/动态库均可），方便多端共享。
  - 同时支持单线程（`Rc`）与 `sync`（`Arc` + `RwLock`）后端。
  - 明确句柄与回调生命周期，避免宿主指针悬挂或重复释放。

## 2. 架构总览

```
 ┌────────────┐       FFI (C ABI)       ┌──────────────┐
 │ 宿主应用层 │◀───────────────────────▶│ mobx_rs runtime │
 └────────────┘                         └──────────────┘
        ▲                                     ▲
        │ mobx_value + 句柄                   │ Atom / Derivation / Runtime
        ▼                                     ▼
     宿主数据模型                        依赖追踪 + Reaction 调度
```

- 宿主通过一组“描述符 + 回调”把字段或计算注册成 observable/computed。
- Rust 继续使用现有的 `Atom`、`Derivation`、`Reaction`、`Runtime`，但不再存储真实业务数据。
- 通过 `u64` 句柄连接宿主与 Rust，两侧互不直接持有对方对象，依赖 registry 做映射。

## 3. 句柄类型

| 句柄              | 说明                                                         |
|------------------|--------------------------------------------------------------|
| `mobx_runtime_t` | 持有 `MobxRuntime` 状态（批处理深度、reaction 队列、策略等）。 |
| `mobx_observable_t` | 宿主注册的 observable 字段。                               |
| `mobx_computed_t`   | 宿主提供 getter/setter 的 computed。                       |
| `mobx_reaction_t`   | autorun/reaction 回调实例。                                 |

实现要点：
- 在 Rust 侧把这些句柄映射到 `Arc<_>`，使用 `DashMap`/`IdRegistry` 管理。
- 暴露 `create/retain/release`，宿主显式管理引用计数。
- 一般一个 runtime 对应一个应用实例，必要时也可以多 runtime 并存。

## 4. 值表示（`mobx_value`）

```c
typedef enum {
    MOBX_VALUE_BOOL,
    MOBX_VALUE_I64,
    MOBX_VALUE_F64,
    MOBX_VALUE_STRING,
    MOBX_VALUE_JSON,
} mobx_value_tag;

typedef struct {
    mobx_value_tag tag;
    union {
        bool    b;
        int64_t i64;
        double  f64;
        struct { const char *ptr; size_t len; } string;
        struct { const uint8_t *ptr; size_t len; } json;
    } data;
} mobx_value;
```

- JSON/二进制作为复杂对象兜底。
- Rust 辅助函数负责 `mobx_value` ↔︎ 内部枚举/原语类型的转换。
- 字符串/二进制的所有权默认由宿主掌握，如需长期保存，Rust 在接收后自行复制。

## 5. Observable 注册

### 宿主侧 API

```c
typedef mobx_value (*mobx_read_cb)(void *user_data);
typedef void (*mobx_write_cb)(void *user_data, const mobx_value *value);

typedef struct {
    const char   *name;
    void         *user_data;
    mobx_read_cb  read;   // 必填
    mobx_write_cb write;  // 选填，宿主也可以手动写入后调用 report_changed
} mobx_observable_desc;

mobx_observable_t mobx_observable_register(
    mobx_runtime_t runtime,
    const mobx_observable_desc *desc);

mobx_value mobx_observable_get(mobx_observable_t observable);
void mobx_observable_set(mobx_observable_t observable, const mobx_value *value);
void mobx_observable_report_changed(mobx_observable_t observable);
void mobx_observable_dispose(mobx_observable_t observable);
```

### 运行时行为

- 注册时创建 `Atom` + 保存回调。
- `mobx_observable_get`：执行 `report_observed()`，然后调用宿主 `read`，这样依赖自动建立。
- `mobx_observable_set`：若提供 `write`，先调用写入再 `report_changed()`；否则宿主自己修改后调用 `report_changed`。
- **调用约定**：每次调用 `mobx_observable_get/set/report_changed` 后，宿主都需要在自身逻辑结束时调用 `mobx_runtime_callback_complete(runtime)`，以便 runtime 决定是否继续 flush reaction。
- `dispose` 清理 registry 条目。

## 6. Computed 注册

```c
typedef struct {
    const char   *name;
    void         *user_data;
    mobx_read_cb  getter;   // 必填
    mobx_write_cb setter;   // 选填
} mobx_computed_desc;

mobx_computed_t mobx_computed_register(
    mobx_runtime_t runtime,
    const mobx_computed_desc *desc);

mobx_value mobx_computed_get(mobx_computed_t computed);
void mobx_computed_set(mobx_computed_t computed, const mobx_value *value);
void mobx_computed_dispose(mobx_computed_t computed);
```

- 需要实现 `AnyComputed` 包装：在 `track_derived_function` 中调用宿主 getter，从而收集依赖。
- getter 返回的字符串/JSON 会被复制到线程本地缓冲区；宿主需在下一次 `mobx_*` 调用之前消费完返回的切片。
- 该缓冲区由 runtime 管理，指针在同一线程上保持有效，直到下一次 FFI 调用重用缓冲区或宿主调用 `mobx_runtime_callback_complete`。
- 没有 setter 时，`mobx_computed_set` 是空操作（只触发 strict-mode 检查）。

## 7. Reaction / Autorun

```c
typedef struct {
    const char       *name;
    mobx_reaction_cb  run;
    void             *user_data;
} mobx_reaction_desc;

mobx_reaction_t mobx_autorun(
    mobx_runtime_t runtime,
    const mobx_reaction_desc *desc);

void mobx_reaction_schedule(mobx_reaction_t reaction);
void mobx_reaction_dispose(mobx_reaction_t reaction);
```

- 默认在 Rust 线程执行 `run`。若 UI 需要主线程，可通过调度器（见 §9）把执行权交还宿主。
- Reaction 内部读取 observable 时会自动记录依赖。

### 回调完成通知

- 所有跨 FFI 的回调（observable read/write、computed getter/setter、reaction/action 闭包）都必须在结束时告知 runtime，以便 core 收尾：

```c
void mobx_runtime_callback_complete(mobx_runtime_t runtime);
```

- 流程：Rust 调用宿主回调前标记 “host-callback-running”；宿主在同步返回前或异步任务结束时调用 `mobx_runtime_callback_complete`。只有调用完成通知后，runtime 才会恢复相应守卫、继续调度。

## 8. Actions 与严格模式

```c
typedef struct {
    const char       *name;
    void             *user_data;
    mobx_reaction_cb  body;
} mobx_action_desc;

void mobx_run_in_action(mobx_runtime_t runtime,
                        const mobx_action_desc *desc);
```

- `mobx_run_in_action` 是 `action(name, || …)` 的 FFI 包装，宿主 `body` 在该闭包里执行，结束后仍需调用 `callback_complete`。
- Guard 风格接口暂不提供，后续按需扩展。

## 9. 调度与线程模型

- FFI 构建默认开启 `sync` feature（`Arc`/`RwLock`），确保多线程安全。
- 调度模式：**Core 驱动的 pull**  
  - 在任何宿主回调完成并调用 `mobx_runtime_callback_complete` 后，runtime 会判断是否需要继续执行 reaction。  
  - 如果需要，runtime 自己调用 `mobx_runtime_flush_reactions(runtime)`，并在 flush 过程中回调宿主 reaction。  
  - 宿主无需主动轮询，也不需要注册 scheduler，只要保证回调完成通知能够及时触发核心即可。
- 若宿主强制要求在特定线程刷新（如 UI 线程），可以在 `callback_complete` 的实现中触发宿主提供的线程切换，再由宿主回调一个专用入口（例如 `mobx_runtime_flush_on_host_thread`），再执行 flush。这部分属于平台 glue，不在 core 范围。
- 仍需保证：同一个 runtime 的 `flush` 只能串行执行，可通过内部互斥或原子标志防止重入。

## 10. 错误处理

```c
typedef enum {
    MOBX_STATUS_OK = 0,
    MOBX_STATUS_INVALID_HANDLE,
    MOBX_STATUS_NULL_CALLBACK,
    MOBX_STATUS_RUNTIME_PANIC,
} mobx_status;
```

- 每个 `extern "C"` 函数返回 `mobx_status`，或在成功时返回句柄。
- FFI 边界使用 `catch_unwind` 捕获 panic，并写入线程局部错误字符串。
- 宿主可通过 `const char* mobx_last_error_message(void);` 读取最近一次错误。

## 11. 实施步骤

1. **新增 `ffi` 模块**  
   - `src/ffi/mod.rs` 内定义句柄、状态码、`mobx_value`、导出函数（`#[no_mangle]`）。  
   - 在 `Cargo.toml` 的 `ffi` feature 下启用 `crate-type = ["cdylib", "rlib"]`。

2. **扩展 runtime/observable API**  
   - 提供回调式 observable/computed 的构造器。  
   - 暴露 handle registry，支持通过 `u64` 查找实体。

3. **改造调度**  
   - `runtime::schedule_reaction_flush` 支持调用宿主 scheduler。  
   - 对外提供 `mobx_runtime_flush_reactions` / `mobx_runtime_has_pending_reactions`。

4. **值转换工具**  
   - 新建 `ffi::value` 模块，集中处理 `mobx_value` ↔︎ Rust 类型的互转。

5. **测试**  
   - 单测覆盖句柄生命周期、错误返回、panic 捕获。  
   - 集成测试：用 `libloading` + mock C 回调模拟宿主侧注册 observable/computed/reaction。

6. **示例与文档**  
   - 编写最小 counter 示例（Swift/Kotlin/Dart 各一份），展示注册字段、autorun、action 的流程。  
   - 补充 README / `docs/ffi_usage.md`，描述线程模型、回调通知、资源释放。

### Swift 宿主示例
- 在 `hosts/swift` 下提供 Swift Package（`MobxRS`），通过 C 头文件 `mobx_rs_ffi.h` 直接链接 Rust 生成的 `libmobx_rs`.
- Swift wrapper 负责：
  - 将 Swift 闭包封装成 `mobx_read_cb` / `mobx_write_cb`，并在回调末尾调用 `mobx_runtime_callback_complete`.
  - 提供 `MobxRuntime`、`MobxObservable` 等类型，简化宿主调用.
  - 链接参数默认指向 `../../target/debug`，也可以通过 `MOBX_RS_LIB_DIR` 环境变量覆盖（脚本会在打包时自动指向 `target/release`）。
- 若只需源码集成，可运行 `scripts/build_rust_artifacts.sh`，它会在 `hosts/swift/Artifacts/` 下产出 macOS / iOS（设备 + 模拟器）的 `libmobx_rs.a` 以及公共头文件，SwiftPM 或 Xcode 只需设置 `MOBX_RS_LIB_DIR` 指向对应目录即可.
- 若需要 XCFramework，可运行 `scripts/build_swift_xcframework.sh [output.zip]`。脚本会执行 `cargo build --release --features ffi`、`swift build --configuration release`，并调用 `swift package archive` 产出 `MobxRS.xcframework.zip`.

- 新增 `mobx_set_enforce_actions(MobxActionPolicy)` 用于控制严格模式（Never / Observed / Always），以及 `mobx_allow_state_changes` / `mobx_allow_state_changes_end` RAII 接口，宿主可以在 Swift 等层面临时允许/禁止修改 observable。

## 12. 决策记录 / 未决事项

- **Observable 快照**：本阶段不做“一次性 dump 全量 observable”；未来会通过实时追踪记录变化。
- **异步写入**：依赖“回调完成通知”机制即可，无需额外延长 action 生命周期。
- **Spy 事件 ID**：需要跨语言稳定的 ID，以便宿主 DevTools 识别同一 observable/computed。
- **调度模式**：Core 在收到回调完成通知后自行判断并调用 `flush`，宿主不主动拉取。若要跳回指定线程，由宿主在 `callback_complete` 阶段完成线程切换后再回调 core。

当前没有其他阻滞项，可以在此基础上进入实现阶段。
