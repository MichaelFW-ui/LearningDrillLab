# Learning Drill Lab

[English](README.md)

Learning Drill Lab 是一个桌面学习工具，用 AI 辅助练习编程概念。输入一个主题，它会生成讲解、练习题、答案评审和追问对话。

目前提示词主要面向中文教学。应用本身使用 Rust 和 Dioxus 编写。

## 功能

- 按主题生成讲解，包含代码例子和常见错误。
- 生成练习题，并在展示前做一次题目校验。
- 评审答案；如果题目本身有问题，正确指出问题也可以算作正确回答。
- 追问对话会带上当前主题、题目和作答记录。
- 本地保存话题历史。
- 可选接入 Bocha 或 Tavily 做联网搜索。
- 可选使用 Jina Reader 读取网页正文。

## 快速开始

```bash
cargo run
```

打开应用里的 **设置**，填写：

- `Base URL`：Chat Completions 兼容接口，例如 `https://api.openai.com/v1`
- `API Key`
- 拉取模型列表后选择模型

可以不填搜索 key。未配置搜索服务时，应用仍然可以作为普通 AI 学习助手使用。

## 可选搜索配置

配置对应服务后，应用会把下面两个工具暴露给模型。

### `web_search`

满足任意一个条件即可启用：

- 填写 Bocha API Key
- 填写 Tavily API Key

如果同时配置 Tavily 和 Bocha，会先用 Tavily。Tavily 失败时自动 fallback 到 Bocha。每次搜索工具调用只发送一个 query，最多返回 10 条结果。

如果使用 Tavily 兼容的中转，请在 `Tavily HTTP Base URL` 填 Tavily 风格 HTTP API 的基础地址。不要填 MCP 地址。直接填到 `/search` 也可以。

### `web_fetch`

默认使用 Jina Reader public endpoint 读取网页正文。

- 由模型选择要读取的 URL。
- 每次最多读取 5 个 URL。
- 多个 URL 串行读取。
- 每个 URL 之间间隔 3 秒。
- 默认不带 Jina API Key。
- 只有 public Reader 返回鉴权或限流响应时，才使用配置的 Jina API Key 重试。
- Jina 不用于搜索。

## 本地数据

应用使用系统配置目录保存状态。设置页会显示 `state.json` 的完整路径。

`state.json` 中会保存：

- API keys
- 当前模型和接口地址
- 话题历史
- 练习题
- 回答和评审记录

这个文件是明文 JSON，请把它当作敏感文件处理。

## 调试日志

AI 调试日志默认关闭。

开启方式：

```bash
LEARNING_DRILL_LAB_AI_DEBUG=1 cargo run
```

开启后，应用会把完整 AI 请求、响应、工具结果、读取到的网页正文和 JSON 修复过程写入本地 `ai-debug.log`。分享日志前请先检查内容。

## 开发

```bash
cargo fmt
cargo check
cargo test
```

## 说明

- 目前还是早期桌面应用，没有做正式打包发布。
- API Key 现在是明文保存。
- UI 和提示词仍然偏个人工作流。
- 搜索和读取工具依赖 Chat Completions 风格的 tool call 支持。
