# Learning Drill Lab

[English](README.md)

Learning Drill Lab 是基于 Tauri 2 的桌面学习工具。输入编程主题后，应用生成讲解和练习，评审答案，并保留追问与本地话题历史。

## 启动与构建

需要 Node.js、Rust 和 Tauri 2 对应平台依赖。首次安装依赖后运行：

```bash
npm ci
npm run tauri dev
```

打包 macOS 应用：

```bash
npm run tauri build -- --bundles app
```

Rust 检查和测试使用 `src-tauri/Cargo.toml`：

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml --all
cargo test --manifest-path src-tauri/Cargo.toml --locked
```

## 使用

在设置页填写模型接口基础地址（`Base URL`）、模型接口密钥（`API Key`），拉取并选择模型。Bocha、Tavily 和 Jina 配置提供可选的联网搜索与网页读取。话题、练习、答案、评审和实验记录保存在设置页显示的本地 `state.json` 中；密钥目前以明文保存，请保护该文件。保存时会生成 `state.json.bak`，主文件损坏时应用会尝试从备份读取。

练习生成由内置的 `curriculum` 技能选择下一步动作和难度。题目经过上下文及可评分性审查。配置远端 LibreCodeInterpreter 后，`experiment-verification` 技能还会设计可执行探针，核对沙箱实际输出；验证状态显示在题目上。练习页的“沙箱实验”允许手动运行完整代码并查看标准输出、标准错误和耗时。

沙箱设置填写服务基础地址与密钥。客户端按 LibreChat 兼容格式向 `/exec` 发送 `code`、`lang`、可选 `session_id`，使用 `x-api-key` 鉴权。支持的语言别名包括 Python、JavaScript、TypeScript、Go、Java、C、C++、PHP、Rust、R、Fortran、D 和 Bash。服务地址留空时，练习保留文本审查结果，验证状态显示为待实验。

## 技能

两个内置技能位于 `src-tauri/skills/`。如需调整教学策略，可在 `state.json` 所在目录下建立 `skills/curriculum/SKILL.md` 或 `skills/experiment-verification/SKILL.md`，使用同名的 YAML frontmatter。应用会优先读取本地技能文件；工具权限、题目数量下限和审查门槛仍由 Rust 代码控制。设置页会显示技能来源。

## 调试

AI 调试日志默认关闭。开发时可用 `LEARNING_DRILL_LAB_AI_DEBUG=1 npm run tauri dev` 开启；日志可能包含完整提示词、响应和网页内容，分享前请检查。
