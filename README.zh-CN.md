# unisolver

[English](README.md) | 简体中文

可嵌入、全离线的天文盲解引擎：给一张星空照片，算出相机指向天空哪里（RA/Dec、
旋转角、视场与完整 WCS），并在图上标出恒星、深空天体、行星与卫星。核心为纯 Rust，
**无网络依赖**，通过 Flutter 插件、C ABI 或 Rust crate 三种方式集成。

- 算法：[tetra3rs](https://github.com/ssmichael1/tetra3rs)（tetra3 / cedar-solve 的
  Rust 移植）的 4 星几何哈希、Wahba/SVD 定姿、统计验证与 WCS 3-DOF 精化，并针对手机
  照片与多档星库做了适配。
- 速度：53 张真实素材端到端 p50 17 ms / p90 313 ms（Apple M2 Max）。
- 平台：iOS、Android（arm64-v8a、x86_64）、macOS、Windows（x64、arm64）。
- 许可：MIT OR Apache-2.0；第三方代码与数据见
  [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md)。

## 快速上手

```bash
# 构建并运行测试
cargo test --workspace --release --features "imageio satellites"

# 命令行解一张图（FITS/XISF/PNG/JPEG/TIFF 自动识别）
zstd -d packages/unisolver_flutter/assets/unisolver_10_80.db.zst -o /tmp/unisolver_10_80.db
cargo run --release -p solvecli -- --db /tmp/unisolver_10_80.db \
  packages/unisolver_flutter/example/assets/sample_scorpius.jpg

# 运行 Flutter 示例 App（macOS / iOS / Android / Windows）
cd packages/unisolver_flutter/example && flutter run
```

集成到自己的 App 请读 **[docs/integration.md](docs/integration.md)**（英文）：三种集成面
的完整 API、初始化、解算、跟踪、标注、标定、数据许可义务与常见坑。构建环境与发布
命令见 [docs/building.md](docs/building.md)。

## 目录结构

```
unisolver/
├── crates/
│   ├── unisolver-core/         # 引擎主体：全部逻辑在此
│   ├── unisolver-cabi/         # C ABI（INDI / ASCOM / 桌面原生 / Python）
│   └── unisolver-synth/        # 测试用合成星场
├── packages/unisolver_flutter/ # Flutter 插件（flutter_rust_bridge 绑定）与示例 App
├── third_party/
│   ├── tetra3/                 # vendored 上游 tetra3rs，由补丁队列生成
│   └── tetra3-patches/         # 对它的本地补丁（docs/upstream.md）
├── tools/
│   ├── solvecli/               # 解算 CLI：批量解算、σ 网格统计、跟踪、标定
│   ├── namesgen/               # 生成多语言名称包（13 种语言）
│   └── xtask/                  # `cargo xtask upstream`：维护补丁队列
├── scripts/
│   ├── ci/                     # 检查：公开文字、Windows 交叉检查、本机清单服务
│   └── verify/                 # 对 astropy 的精度核验
├── testdata/                   # 测试输入的来源说明
└── docs/                       # 集成与构建指南
```

### crates/unisolver-core

行为的**唯一来源**；Flutter 与 C 两层只做封送。

| 模块 | 职责 |
|---|---|
| `solver.rs` | 入口：`Solver`、`SolveOptions`、提取档位、FOV 梯子（`aspect_ladder` / `presets_with_hints` / `solve_with_fov_presets`），并按星库范围裁剪 |
| `outcome.rs` | 结果：`SolveOutcome` / `SolvedGeometry` / `Wcs`（像素 ↔ 天球以 WCS 为准） |
| `imageio/` | 按魔数分发格式：`fits.rs`（含 NAXIS3=3 彩色）、`xisf.rs`（zlib/lz4/zstd + shuffle）、`raster.rs`（PNG/JPEG/TIFF，含 16-bit） |
| `annotate.rs` | 标注层：恒星、命名星、深空天体、太阳系、**卫星**，各层带可用性与原因 |
| `pool.rs` | **多库路由**：`SolverPool` 注册多档星库，按 FOV 分派、跨档降级，提取只做一次 |
| `names_pack.rs` | 多语言名称包（`UNAM`）：语言集由数据决定，回退链 请求语言 → 英文 |
| `ephemeris.rs`、`satellites.rs` | 行星与月亮历表（Standish + Meeus）、卫星过境（TLE + SGP4） |
| `calibrate.rs`、`camera.rs` | 端上多帧标定（径向 / 多项式畸变）与相机模型 |
| `names.rs` + `named_stars.csv` | IAU 411 颗命名星（HIP、位置、星等、英文名；本地化名在名称包里） |
| `dso.rs`、`coords.rs`、`quat.rs`、`frame.rs`、`aberration.rs` | DSO 表、坐标换算、四元数、帧（含行距）、光行差 |

测试在 `crates/unisolver-core/tests/`：`solve_test`（梯子、档位、裁剪）、`pool_test`
（路由）、`storage_test`（UNISOLV2 mmap 等价与容错）、`annotate_test`、`calibrate_test`，
以及依赖私有实拍素材的测试——素材不在时打印 `skipped` 并通过（见
[testdata/README.md](testdata/README.md)）。

### 约定（改代码前必读）

1. **坐标系**：一律图像**左上角原点**、+x 右、+y 下；tetra3 内部的中心原点坐标在 core
   边界完成转换。
2. **FOV 指水平方向**（沿宽度）。竖拍时水平边是短边：手机主摄约 44–48°，不是 70°+。
3. **姿态**：像素 ↔ 天球一律走 `solution.wcs`；`quat_icrs2cam_wxyz` 是 SVD 阶段的姿态，
   与最终 WCS 可差数角分（上游行为），只作跟踪 hint。
4. **头信息只是提示**：FITS/EXIF 头可能缺失、写错（减焦镜、binning）或语义不一，只用来
   调整梯子顺序，失败后梯子必须照走。
5. **σ 不写死**：`PhoneJpeg`（σ10）/ `CleanSensor`（σ5）/ `Auto`（σ10 先行，失败换档）/
   `Custom`，取值都来自真实素材的实测。

### third_party/tetra3

上游 **v0.13.0** 加上 [third_party/tetra3-patches/](third_party/tetra3-patches/README.md)
里的补丁队列：两条 Cargo.toml 调整，以及 `UNISOLV2` mmap 存储（`src/solver/storage.rs`，
把模式表留在磁盘按需分页，深库常驻内存从 1.1 GB 降到 124 MB）。这棵树是生成的：只能经
`cargo xtask upstream edit` / `export` 修改，动手前先读 rebase 注意事项
（[docs/upstream.md](docs/upstream.md)）。

### packages/unisolver_flutter

| 路径 | 用途 |
|---|---|
| `lib/unisolver_flutter.dart` | 对外导出 |
| `lib/asset_installer.dart` | 首启安装随包资产（幂等；自动迁移旧格式星库） |
| `lib/src/db_manager.dart` | 星库获取：清单 → 断点续传 → sha256 → 解压 → 注册 |
| `lib/src/rust/` | **flutter_rust_bridge 生成，勿手改**（改 `rust/src/api/**` 后重新生成） |
| `rust/src/api/` | 绑定层：`solver.rs`（单库与多库句柄）、`types.rs`（DTO）、`install.rs`（解压 + sha256）、`satellites.rs`、`logging.rs` |
| `assets/` | 随包 10–80° 星库（zstd 16 MB）与 DSO 表（含轮廓，771 KB） |
| `lib/optional/` | 多语言名称包（215 KB，GPL-2.0-or-later），由 App 自行选择是否带上 |
| `example/` | 示例 App：解算、实时跟踪、标定、星库管理四页，可切换标注语言 |

### 星库

插件随包提供 **10–80° 宽场星库**（手机与广角镜头）。长焦与望远镜可用上游 tetra3rs
工具生成更窄视场的星库：引擎直接加载，对你注册的所有档位做路由（`SolverPool`），
`DbManager` 能从任何提供清单的静态服务器安装。见
[docs/integration.md](docs/integration.md) 第 1.6 节。

## 开发

```bash
git config core.hooksPath .githooks     # 一次：提交信息与推送前检查
cargo test --workspace --release --features "imageio satellites"
cargo clippy -p unisolver-core -p unisolver-synth -p unisolver-cabi -p namesgen -p solvecli -p xtask \
  --all-targets --features "imageio satellites" -- -D warnings
python3 scripts/ci/check_public_text.py
bash scripts/ci/check_windows.sh
cargo xtask upstream check
(cd packages/unisolver_flutter && flutter test && flutter analyze)
```

提交信息只写一行 `type(scope): summary`（type：feat fix perf refactor docs test data ci
chore revert），不写正文与 trailer，由钩子强制。用户可感知的改动在 `CHANGELOG.md` 记一条。发版与 GitHub 镜像见 [docs/releasing.md](docs/releasing.md)。
代码注释与对外文档使用英文。

改动落点：

| 要做的事 | 改哪里 |
|---|---|
| 解算策略、提取档位、FOV 梯子 | `crates/unisolver-core/src/solver.rs` |
| 支持新图像格式 | `crates/unisolver-core/src/imageio/` + 魔数分发 |
| 新标注图层 | `crates/unisolver-core/src/annotate.rs` |
| Flutter 新 API | `packages/unisolver_flutter/rust/src/api/`，然后重新生成绑定 |
| C ABI 新符号 | `crates/unisolver-cabi/src/lib.rs`（cbindgen 重出头文件） |
| 上游算法本身 | `third_party/tetra3-patches/` 里的补丁（`cargo xtask upstream edit`，见 docs/upstream.md） |

## 许可

按你的选择适用 [MIT](LICENSE-MIT) 或 [Apache-2.0](LICENSE-APACHE)。
Copyright (c) 2026 Suzhou UMa Technology Co., Ltd.

随包数据另有条款：星库派生自 ESA Gaia DR3（CC BY-SA 3.0 IGO，须署名），DSO 表派生自
OpenNGC（CC BY-SA 4.0），名称包派生自 Stellarium（GPL-2.0-or-later）。详见
[THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md)。
