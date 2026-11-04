# MDeditor

本地 Markdown 阅读与排版编辑器（Windows 桌面应用，Tauri 2 构建）。

## 下载

发布包统一通过 [Releases](https://github.com/xingyi-x/MDeditor/releases) 分发；仓库 `dist/` 里另附一份绿色版 `MDeditor.exe`，方便 clone 后直接运行（体积较大，会随版本更新覆盖）。

| 文件 | 说明 |
| --- | --- |
| `MDeditor-v1.3-setup.exe` | **安装版（推荐）**：含 `.md` 文件关联、开始菜单快捷方式，可在「应用和功能」中卸载 |
| `MDeditor.exe` | **绿色版**：单文件免安装，双击即用（前端资源已内嵌），同时作为命令行入口 |
| `mdconv.bat` | 命令行转换包装脚本，与 `MDeditor.exe` 放同一目录即可使用 |

> 本机构建产出的安装包名为 `MDeditor-v1.3-安装包.exe`；上传 Releases 时 GitHub 会剔除文件名中的非 ASCII 字符，故下载名为 `MDeditor-v1.3-setup.exe`，两者内容一致。

环境要求：Windows 10/11 x64 + WebView2 Runtime（Windows 11 已内置；Win10 若缺失需[单独安装](https://developer.microsoft.com/microsoft-edge/webview2/)）。

**特点：**

- 纯本地运行，无账号、无上传、无遥测
- 源码 / 分栏 / 预览 / 阅读 四种视图，编辑与预览同步滚动
- **主题系统**：10 款内置文档主题（清爽白 / 暖黄护眼 / 豆沙绿 / 暗夜黑 / 仿 Word / 论文衬线 / 书卷衬线 / 极简灰 / 蓝图科技 / 樱粉），支持自定义主题编辑器（颜色 + 字体 + 附加 CSS），可导出为主题文件；附加 CSS 在应用内预览中也生效
- **Word/PDF 式排版**：A4/A5/B5/Letter/16K 纸张、页边距（mm）、行距、段间距、首行缩进、两端对齐、正文/标题/代码字体、标题倍率、内容宽度；纸张模式下文档显示为真实比例的"纸"
  - 默认按「阅读优先」留白：流式模式四周 56px、A4 页边距 24/20mm，正文宽度 860px
  - 行距 / 首行缩进 / 对齐方式默认「跟随主题」，换主题即换版式（如「仿 Word」自带 2em 缩进 + 两端对齐）；手动改过之后以手动值为准
- **分页预览**（paged.js）：按纸张真实分页显示
- **导出**：PDF（直接生成文件，**无浏览器页眉页脚**）、自包含 HTML（图片与公式字体内嵌）、Word（扩展名为 `.doc` 的 HTML，版式已扁平化）、纯文本（去掉 Markdown 记号）。
  打印与 PDF 导出按「打印友好」处理：正文、代码块、引用、表头会被强制成白底黑字，因此**彩色主题的配色不会出现在成品 PDF 里**；Word 的 HTML 对 `data:` 图片和 SVG（Mermaid）支持有限，以文字内容为准
- **渲染能力**：GFM、任务列表（预览中可直接勾选并回写源码）、表格、代码高亮（highlight.js）、数学公式（KaTeX）、Mermaid 图表（懒加载）
- **文件管理**：文件夹树浏览、最近打开、全局全文搜索、重命名/删除、在资源管理器中显示
- **拖拽打开**：把文件直接拖到窗口上即可（有「松开即打开」提示层）——  
  `.md` / `.txt` 直接打开，PDF / Word / Excel / PPT / HTML 自动转成 Markdown 后打开，图片插入当前文档
- 图片：粘贴 / 拖入自动保存到文档旁 `assets/` 目录（可改为内嵌 base64）
- 自动保存、打字机滚动、编辑器字号调节、GBK 编码自动识别；打开 GBK / UTF-16 文件时保存会保留原编码、BOM 与换行风格，UTF-8 中偶发坏字节不再导致整篇被误判成 GBK
- 支持 `.md` / `.markdown` 文件关联（安装版会注册关联；也可在「设置 → 文件关联」一键登记，并跳到 Windows 默认应用页确认）
- **文档转 Markdown**：PDF / Word(docx) / Excel(xlsx) / PPT(pptx) / HTML / RTF / CSV → Markdown，  
  图形界面与命令行都能用（见下节）

## 文档转 Markdown

把 PDF / Word / Excel / PPT / HTML 等**转成适合 AI 阅读的 Markdown**：保留标题层级、表格、列表、  
图片引用与代码块，丢掉字体样式噪音，并把 PDF 里被排版切断的行重新接成完整段落。

### 图形界面

- 菜单「文件 → 导入为 Markdown…」（`Ctrl+Alt+O`）
- 或把 PDF / docx / xlsx / pptx 等文件**直接拖到窗口里**

转换结果写在源文件同目录（同名 `.md`），图片导出到旁边的 `<名称>_assets/`，转换完自动打开。

### 命令行（cmd / PowerShell / 批处理都能调）

程序是 GUI 子系统的 exe（双击不弹黑框），但**从控制台调用时会自动附加到父控制台**，  
所以 `mdeditor.exe --to-md ...`、`> 文件` 重定向、管道都能正常用。

```bat
:: 单个文件 → 同目录同名 .md
mdeditor.exe --to-md 报告.pdf

:: 整个目录批量转（可递归），输出到指定目录
mdeditor.exe --to-md D:\docs -r -d D:\docs\md

:: 只打印结果、不落盘（可重定向；此模式下不导出图片）
mdeditor.exe --to-md 论文.docx --stdout > 论文.md

:: 只要纯文本，不要图片和元信息
mdeditor.exe --to-md 手册.pdf --no-images --quiet

:: 加 YAML 元信息，喂给 AI / RAG 时便于溯源
mdeditor.exe --to-md 手册.pdf --front-matter -o 手册.md
```

选项：

| 选项                 | 说明                           |
| ------------------ | ---------------------------- |
| `-o <文件>`          | 指定输出 .md 路径（单文件模式）           |
| `-d <目录>`          | 输出到指定目录（批量默认与源文件同目录）         |
| `-r`               | 目录模式递归子目录                    |
| `--force`          | 覆盖已存在的 .md（默认自动改名，不覆盖）       |
| `--stdout`         | 只打印到标准输出，不写文件（仅单文件，自动关闭图片导出） |
| `--no-images`      | 不提取图片                        |
| `--front-matter`   | 开头写入 YAML 元信息（标题 / 来源 / 时间）  |
| `--quiet`          | 静默，只输出生成的文件路径（便于脚本取用）        |
| `-h` / `--version` | 帮助 / 版本                      |

支持格式：`pdf` `docx` `doc`（需本机装 Word/WPS，走 COM 转换）`xlsx` `xlsm` `pptx`  
`html` `htm` `txt` `csv` `tsv` `rtf` `md`

> 扫描版 PDF（图片型）提取不到文字，需先做 OCR。

**随取随用的两种方式：**

1. 用 `dist\mdconv.bat`：`mdconv.bat 报告.pdf` 等价于 `mdeditor.exe --to-md 报告.pdf`。  
   （若想在任意位置调用，把 `dist` 目录加进系统 PATH。）
2. 安装版装好后，主程序在 `%LOCALAPPDATA%\MDeditor\MDeditor.exe`，  
   把它所在目录加进 PATH 后可直接 `mdeditor --to-md 报告.pdf`。

### 直接导出 PDF（命令行 / 批处理）

```bat
:: 打开文档 → 导出 PDF → 自动退出。不弹打印对话框，成品不带浏览器页眉页脚
mdeditor.exe 报告.md --pdf-out D:\out\报告.pdf
```

纸张尺寸与页边距取自当前设置（A4/A5/B5/Letter/16K）。导出按打印样式强制白底黑字，与彩色预览的配色不完全一致；进程退出码 0 表示成功、1 表示失败，批处理可用 `%ERRORLEVEL%` 判断。

## 目录结构

```
MDeditor/
├── app/                 前端（纯 HTML/CSS/JS，无构建步骤）
│   ├── index.html
│   ├── css/             app(UI) / doc(渲染) / print(打印) 三层样式
│   ├── js/              core / fsx / md / editor / theme / panels / actions / app
│   └── vendor/          全部第三方库本地化（CodeMirror5、marked、DOMPurify、
│                        highlight.js、KaTeX、Mermaid、paged.js）
├── src-tauri/           Tauri 2 后端（Rust，自定义文件命令，支持任意路径读写）
│   └── src/
│       ├── main.rs       文件读写 / 目录 / 搜索 / 转换命令
│       ├── cli.rs        命令行模式：--to-md 文档转 Markdown
│       └── convert/      转换引擎：docx / xlsx / pptx / pdf / html / rtf / plain
├── dist/               打包产物
│   ├── MDeditor.exe              绿色版 / 命令行入口（仓库内附带）
│   ├── MDeditor-v1.3-安装包.exe    NSIS 安装包（仅本机构建产物，Releases 下载名为 setup）
│   └── mdconv.bat                命令行包装脚本
├── _toolchain/          Rust 工具链（仅本机自用，可选，gitignore）
├── build-release.bat    一键构建 release exe
├── LICENSE              MIT
└── 示例文档.md           功能演示文档
```

## 构建

环境要求：Rust stable（MSVC 工具链）、MSVC Build Tools、WebView2 Runtime（Node 可选；前端无构建步骤）。

```bat
build-release.bat
```

产物：`src-tauri\target\release\mdeditor.exe`（单文件绿色版，前端资源已内嵌）。

- `build-release.bat` **只在仓库内存在 `_toolchain\cargo\bin\cargo.exe` 时**才把 `CARGO_HOME` / `RUSTUP_HOME` 指向它；
  没有该目录时直接使用系统已安装的 Rust 工具链，clone 后无需额外配置即可构建。
- `.cargo/config.toml` 默认不强制 crates.io 镜像。国内网络如需加速，自行加入：

```toml
[source.crates-io]
replace-with = "rsproxy-sparse"
[source.rsproxy-sparse]
registry = "sparse+https://rsproxy.cn/index/"
```

如需安装包（NSIS，含 .md 文件关联与开始菜单快捷方式），安装 tauri-cli 后运行：

```
cargo install tauri-cli --version ^2
cargo tauri build
```

## 快捷键

| 快捷键                            | 功能                      |
| ------------------------------ | ----------------------- |
| Ctrl+S / Ctrl+Shift+S          | 保存 / 另存为                |
| Ctrl+N / Ctrl+O / Ctrl+Shift+O | 新建 / 打开文件 / 打开文件夹       |
| Ctrl+Alt+O                     | 导入 PDF/Word 等为 Markdown |
| Ctrl+B / Ctrl+I / Ctrl+K       | 加粗 / 斜体 / 链接            |
| Ctrl+1..4 / Ctrl+Shift+Q       | 一至四级标题 / 引用            |
| Ctrl+F / Ctrl+H                | 编辑器内查找 / 替换             |
| Alt+1..4                       | 源码 / 分栏 / 预览 / 阅读       |
| Ctrl+\\                        | 显示/隐藏侧栏                 |
| Ctrl+P                         | 导出 PDF（无页眉页脚，直接落盘）      |
| Ctrl+= / Ctrl+-                | 编辑器字号                   |
| Ctrl+滚轮 / Ctrl+0             | 画面缩放 / 回到 100%          |
| F11                            | 全屏                      |
| Esc                            | 退出阅读模式 / 关闭面板            |

## 更新日志

### v1.3（版本号已统一：Cargo.toml / tauri.conf.json / README / 「设置 → 关于」/ 安装包名均为 v1.3.0）

按代码审查报告做的集中修复，覆盖前端、后端、转换引擎与安全配置：

- **数据安全**：打开/新建文档清空撤销历史；异步保存期间继续输入不再被误标「已保存」；关闭窗口拦截未保存内容；读取时保留 GBK / UTF-16 的编码、BOM 与换行并在保存时还原；写盘改为原子替换；偶发坏字节不再让整篇 UTF-8 被误判成 GBK。
- **前端**：修复「恢复上次文件夹」抛异常、Ctrl+Shift+S/O 失效、页内锚点无法跳转、任务勾选按行号回写、图片相对路径误判、附加 CSS 在预览中不生效、导出 HTML 里写死他人作者信息。
- **后端**：耗时的转换 / 搜索 / 注册表命令移到后台线程，不再卡住窗口；修复注册表兜底路径、控制台闪窗、`--pdf-out` 输出路径被当成启动文件、默认应用打开时的引号处理；`--pdf-out` 支持真实退出码。
- **转换引擎**：修 PDF 中文括号编号丢字、图片解码上限与 Indexed 调色板 / PNG predictor、加密 PDF 图片缺失、表格向上回扫越界；修 RTF 中文字体名混入正文与汉字重复、DOCX 嵌套表格与 run 合并、HTML 单元格错位与引用/有序列表、PPTX 幻灯片顺序与备注/表格、RTF/DOCX 的 `.doc` 转换（UTF-8 BOM、Word 进程回收、路径不再拼进 PowerShell）；`.md` 输入原样透传、代码块内容不再被整理规则改写。
- **安全**：启用严格 CSP（`script-src 'self'`，移除内联脚本）；写盘命令拒绝写入启动目录与可执行文件；文件命令改后台执行并保留 panic 捕获的线程安全；`.doc` 转换的路径改用环境变量传递。

已知限制：CMYK 编码的 JPEG 仍按原样落盘（颜色可能反相）；JPX / CCITT 图像滤镜暂不支持；删除文件仍是永久删除（不进回收站）；单实例、外部修改检测与自动化测试尚未加入。

### v1.2

- **修复**：导出 PDF 的成品带有浏览器页眉（日期 + 文档标题）与页脚（`tauri.localhost` + 页码）。
  原先走 `window.print()`，那个开关只能由用户在打印预览里手动取消；现改为直接调用
  WebView2 的 `PrintToPdf` 并显式 `ShouldPrintHeaderAndFooter=false`，同时按所选纸张设置页面尺寸与页边距。
- **修复**：导出 Word（.doc）丢失全部版式。Word 的 HTML 引擎不支持 CSS 自定义属性，
  原先把 `var(--doc-*)` 原样写出，字体、字号、行距、缩进、对齐全部失效；
  现导出前把变量扁平化为字面值，并写入 `@page WordSection1` 页面设置。
- **修复**：导出纯文本等于把 `.md` 源码换个扩展名。现按 Markdown 结构拍平：
  去掉记号，保留标题层级、列表缩进、表格（制表符分隔）、代码块原样，图片降级为 `[图片：alt]`。
- **新增**：命令行 `--pdf-out <路径>`，无交互导出 PDF（便于批处理与自测）。
- **新增**：仓库直接附带可运行的 `dist/MDeditor.exe`，clone 后无需构建即可使用。

### v1.1

首个公开版本。

## 许可证

本项目基于 [MIT License](LICENSE) 发布，Copyright (c) 2026 xingyi-x。

`app/vendor/` 下的第三方库（CodeMirror 5、marked、DOMPurify、highlight.js、KaTeX、Mermaid、
paged.js）各自保留其原始许可证，具体条款见各库文件头注释或上游仓库。
