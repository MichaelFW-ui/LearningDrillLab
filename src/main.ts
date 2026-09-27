import "./styles.css";
import "katex/dist/katex.min.css";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import DOMPurify from "dompurify";
import { marked } from "marked";
import renderMathInElement from "katex/contrib/auto-render";

type Settings = { base_url: string; api_key: string; selected_model: string; available_models: string[]; bocha_api_key: string; tavily_api_key: string; tavily_base_url: string; jina_api_key: string; sandbox_base_url: string; sandbox_api_key: string };
type Review = { is_correct: boolean; score: number; summary: string; mistakes: string[]; corrected_answer: string; next_steps: string[] };
type Exercise = { id: string; kind: string; title: string; prompt: string; starter_code: string; hints: string[]; difficulty: string; verification?: { status: string; note: string; code: string; stdout: string; stderr: string } | null };
type Topic = { id: string; title: string; created_at: string; updated_at: string; messages: { role: "User" | "Assistant"; content: string; raw_response: string | null }[]; concept: { title: string; language: string; summary: string; key_points: string[] } | null; exercises: Exercise[]; selected_exercise_id: string | null; attempts: { exercise_id: string; review: Review | null }[]; experiment_prompts: { title: string; prompt: string }[]; experiment_runs: { exercise_id: string; code: string; lang: string; status: string; stdout: string; stderr: string; elapsed_ms: number; session_id: string | null; files: unknown[] }[] };
type AppState = { settings: Settings; topics: Topic[]; active_topic_id: string | null; topic_sort: "UpdatedDesc" | "CreatedDesc" | "TitleAsc"; status?: string | null };

const root = document.querySelector<HTMLDivElement>("#app");
if (!root) throw new Error("缺少应用根节点");
let state: AppState | null = null;
let settings: Settings | null = null;
let page: "workspace" | "settings" = "workspace";
let busy = false;
let cancellable = false;
let cancelRequested = false;
let status = "正在加载本地历史记录…";
let activity: string[] = [];
let error = "";
let chatDraft = "";
let answerDraft = "";
let experimentDraft: string | null = null;
let experimentLanguage = "";
let renameId: string | null = null;
let renameDraft = "";
let showKeys = false;
let storagePath = "";
let skills: { name: string; description: string; source: string }[] = [];

function esc(value: unknown): string {
  return String(value ?? "").replace(/[&<>"']/g, (char) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[char] ?? char);
}
function md(value: string): string {
  return DOMPurify.sanitize(marked.parse(value, { async: false, gfm: true, breaks: true }) as string);
}
function list(items: string[]): string { return items.map((item) => `- ${item.replace(/\n/g, "\n  ")}`).join("\n"); }
function topic(): Topic | null { return state?.topics.find((item) => item.id === state?.active_topic_id) ?? null; }
function exercise(item: Topic | null): Exercise | null { return item?.exercises.find((entry) => entry.id === item.selected_exercise_id) ?? null; }
function date(value: string): string { return new Date(value).toLocaleString("zh-CN", { hour12: false }); }
function learned(item: Topic | null): boolean { return Boolean(item && (item.concept || item.exercises.length || item.messages.some((entry) => entry.role === "Assistant" && entry.raw_response))); }
function topicItems(): Topic[] {
  const items = [...(state?.topics ?? [])];
  if (state?.topic_sort === "TitleAsc") return items.sort((a, b) => a.title.localeCompare(b.title, "zh-CN"));
  if (state?.topic_sort === "CreatedDesc") return items.sort((a, b) => b.created_at.localeCompare(a.created_at));
  return items.sort((a, b) => b.updated_at.localeCompare(a.updated_at));
}
function topicsHtml(): string {
  return topicItems().map((item) => item.id === renameId
    ? `<div class="topic-rename"><input data-field="rename" value="${esc(renameDraft)}"><button data-action="rename-save">保存</button><button data-action="rename-cancel">取消</button></div>`
    : `<div class="topic-item"><button class="topic-open ${item.id === state?.active_topic_id ? "selected" : ""}" data-action="switch" data-id="${item.id}"><span class="topic-title">${esc(item.title)}</span><span class="topic-meta">${esc(date(item.updated_at))}</span></button><div class="topic-tools"><button class="topic-tool" data-action="rename-start" data-id="${item.id}">改</button><button class="topic-tool" data-action="delete" data-id="${item.id}">删</button></div></div>`).join("");
}
function chatHtml(item: Topic | null): string {
  const hasContent = learned(item);
  const messages = item?.messages.length ? item.messages.map((message) => `<div class="message ${message.role === "User" ? "user" : "assistant"} markdown-content">${md(message.content)}</div>`).join("") : `<div class="message assistant">输入一个想学习的编程语言知识点，例如 Rust 所有权借用、TypeScript 泛型约束、Python 装饰器。AI 会讲解并生成练习。</div>`;
  return `<section class="chat-panel"><div class="panel-header"><div class="composer-row"><button class="primary" data-action="new">开新话题</button><select data-field="sort"><option value="UpdatedDesc" ${state?.topic_sort === "UpdatedDesc" ? "selected" : ""}>最近更新</option><option value="CreatedDesc" ${state?.topic_sort === "CreatedDesc" ? "selected" : ""}>最近创建</option><option value="TitleAsc" ${state?.topic_sort === "TitleAsc" ? "selected" : ""}>标题 A-Z</option></select></div><span class="muted">历史话题</span></div><div class="topic-list">${topicsHtml()}</div><div class="messages">${messages}</div><div class="composer"><textarea data-field="chat" placeholder="${hasContent ? "追问或质疑当前讲解..." : "输入想学习的知识点..."}">${esc(chatDraft)}</textarea><div class="composer-row">${hasContent ? `<button data-action="regenerate" ${busy ? "disabled" : ""}>重新生成练习</button>` : ""}<button class="primary" data-action="${hasContent ? "follow-up" : "generate"}" ${busy ? "disabled" : ""}>${hasContent ? "追问" : "生成练习"}</button></div></div></section>`;
}
const verificationNames: Record<string, string> = { verified: "实验一致", contradicted: "实验矛盾", unavailable: "实验不可用", unverified: "待实验验证", not_applicable: "无法自动实验", inconclusive: "实验证据不足" };
const kindNames: Record<string, string> = { FillBlank: "填空题", FixBug: "改错题", WriteFromScratch: "从零写代码", PredictCompileResult: "预测编译结果" };
function drillHtml(item: Topic | null): string {
  if (!item) return `<section class="drill-panel"></section>`;
  const concept = item.concept ? `<div class="section"><h2>${esc(item.concept.title)}</h2><p class="muted">${esc(item.concept.language)}</p><div class="markdown-body markdown-content">${md(item.concept.summary)}</div>${item.concept.key_points.length ? `<div class="markdown-body markdown-content">${md(list(item.concept.key_points))}</div>` : ""}</div>` : `<div class="section"><h2>当前知识点</h2><p class="muted">还没有生成知识点。请先在左侧输入学习目标并发送。</p></div>`;
  const tabs = `<div class="section"><div class="exercise-tabs">${item.exercises.map((entry) => `<button class="exercise-tab ${entry.id === item.selected_exercise_id ? "selected" : ""}" data-action="select" data-id="${entry.id}">${kindNames[entry.kind] ?? esc(entry.kind)}</button>`).join("")}</div>${item.exercises.length ? `<div class="composer-row"><button data-action="regenerate" ${busy ? "disabled" : ""}>重新生成练习</button></div>` : ""}</div>`;
  const selected = exercise(item);
  if (!selected) return `<section class="drill-panel">${concept}${tabs}<div class="section"><h2>练习区</h2><p class="muted">AI 生成练习后会显示在这里。</p></div></section>`;
  const prompt = selected.starter_code.trim() ? selected.prompt.replace(selected.starter_code.trim(), "").replace(/```[\s\S]*?```/g, "").replace(/\n{3,}/g, "\n\n").trim() : selected.prompt;
  const review = item.attempts.filter((attempt) => attempt.exercise_id === selected.id).at(-1)?.review;
  const experiment = item.experiment_prompts.at(-1);
  const run = item.experiment_runs?.filter((entry) => entry.exercise_id === selected.id).at(-1);
  const code = experimentDraft ?? selected.starter_code ?? "";
  const language = experimentLanguage || item.concept?.language || "py";
  return `<section class="drill-panel">${concept}${tabs}<div class="section"><h2>${esc(selected.title)}</h2><p class="muted">${kindNames[selected.kind] ?? esc(selected.kind)} · ${esc(selected.difficulty)}</p>${selected.verification ? `<p class="muted"><strong>验证状态：</strong>${esc(verificationNames[selected.verification.status] ?? selected.verification.status)}。${esc(selected.verification.note)}</p>${selected.verification.code ? `<details class="verification-details"><summary>查看自动实验代码与输出</summary><pre class="code-block">${esc(selected.verification.code)}</pre><h4>标准输出</h4><pre class="code-block">${esc(selected.verification.stdout || "（空）")}</pre><h4>标准错误</h4><pre class="code-block">${esc(selected.verification.stderr || "（空）")}</pre></details>` : ""}` : ""}<div class="markdown-body markdown-content">${md(prompt)}</div>${selected.starter_code.trim() ? `<pre class="code-block">${esc(selected.starter_code)}</pre>` : ""}${selected.hints.length ? `<h3>提示</h3><div class="markdown-body markdown-content">${md(list(selected.hints))}</div>` : ""}</div><div class="section"><h3>你的答案</h3><textarea class="answer-box code-input" data-field="answer" spellcheck="false" autocomplete="off" wrap="off" placeholder="在这里写答案或代码...">${esc(answerDraft)}</textarea><div class="composer-row"><button data-action="experiment" ${busy ? "disabled" : ""}>生成实验方案</button><button class="primary" data-action="submit" ${busy ? "disabled" : ""}>提交答案</button></div></div><div class="section"><h3>沙箱实验</h3><p class="muted">在远端隔离环境运行代码，记录实际输出。</p><div class="composer-row"><input data-field="experiment-language" value="${esc(language)}" aria-label="实验语言" placeholder="py / js / cpp / rs"><button data-action="execute-experiment" ${busy ? "disabled" : ""}>运行代码</button></div><textarea class="answer-box code-input" data-field="experiment-code" spellcheck="false" placeholder="输入待验证的完整代码">${esc(code)}</textarea>${run ? `<div class="experiment-box"><p><strong>状态：</strong>${esc(run.status)} · ${esc(run.elapsed_ms)} ms · ${esc(run.lang)}</p><h4>标准输出</h4><pre class="code-block">${esc(run.stdout || "（空）")}</pre><h4>标准错误</h4><pre class="code-block">${esc(run.stderr || "（空）")}</pre></div>` : ""}</div>${review ? `<div class="section"><h3>答案评审结果</h3><div class="review-box"><p><strong>结论：</strong>${review.is_correct ? "正确" : "需要修正"} · ${review.score}/100</p><div class="markdown-body markdown-content">${md(review.summary)}</div>${review.mistakes.length ? `<h3>问题</h3><div class="markdown-body markdown-content">${md(list(review.mistakes))}</div>` : ""}<h3>修正答案</h3><pre class="code-block">${esc(review.corrected_answer)}</pre>${review.next_steps.length ? `<h3>下一步练习</h3><div class="markdown-body markdown-content">${md(list(review.next_steps))}</div>` : ""}</div></div>` : ""}${experiment ? `<div class="section"><h3>实验 Prompt</h3><div class="experiment-box"><p><strong>${esc(experiment.title)}</strong></p><pre class="code-block">${esc(experiment.prompt)}</pre></div></div>` : ""}</section>`;
}
function settingField(label: string, key: keyof Settings, hint = "", secret = false): string {
  const value = settings?.[key] ?? "";
  return `<label for="setting-${key}">${esc(label)}</label><input id="setting-${key}" data-setting="${key}" type="${secret && !showKeys ? "password" : "text"}" value="${esc(value)}" placeholder="${esc(hint)}">`;
}
function settingsHtml(): string {
  return `<div class="settings-page"><div class="settings-card"><h2>API 设置</h2><div class="warning">当前为开发版本：API Key 会以明文存储在本地配置文件中。请只在可信设备上使用。</div><div class="form-grid">${settingField("Base URL", "base_url", "https://api.openai.com/v1")}${settingField("API Key", "api_key", "sk-...", true)}${settingField("博查 API Key", "bocha_api_key", "sk-...", true)}${settingField("Tavily API Key", "tavily_api_key", "tvly-...", true)}${settingField("Tavily HTTP Base URL", "tavily_base_url", "https://api.tavily.com")}${settingField("Jina API Key（限流备用）", "jina_api_key", "jina_...", true)}${settingField("沙箱服务地址", "sandbox_base_url", "https://host:8443")}${settingField("沙箱 API Key", "sandbox_api_key", "x-api-key", true)}<label for="setting-model">模型</label><select id="setting-model" data-setting="selected_model">${settings?.available_models.length ? settings.available_models.map((model) => `<option value="${esc(model)}" ${model === settings?.selected_model ? "selected" : ""}>${esc(model)}</option>`).join("") : `<option value="">请先拉取模型列表</option>`}</select></div><div class="form-actions"><button data-action="toggle-keys">${showKeys ? "隐藏 Key" : "显示 Key"}</button><button data-action="fetch-models" ${busy ? "disabled" : ""}>拉取模型列表</button><button class="primary" data-action="save-settings" ${busy ? "disabled" : ""}>保存设置</button></div><p class="muted">沙箱地址只填服务地址，API Key 在单独的输入框填写。健康检查路径无需填写。</p><p class="muted">Bocha 或 Tavily API Key 可启用联网搜索；Jina Reader 默认无 Key 读取网页。</p><p class="muted">启用技能：${skills.map((skill) => `${esc(skill.name)}（${esc(skill.description)}，${esc(skill.source)}）`).join("；") || "正在读取…"}</p><p class="muted">本地配置文件：${esc(storagePath)}</p></div></div>`;
}
function render(): void {
  if (!root) return;
  root.innerHTML = `<div class="app-shell"><div class="topbar"><div class="brand"><h1>Learning Drill Lab</h1><span>编程语言结构记忆训练</span></div><div class="top-actions"><button class="ghost" data-action="workspace">工作台</button><button class="ghost" data-action="settings">设置</button></div></div>${error || status ? `<div class="status-line ${error ? "error" : ""}">${esc(error || status)}${busy && cancellable ? `<button data-action="cancel-task" ${cancelRequested ? "disabled" : ""}>${cancelRequested ? "正在取消…" : "取消任务"}</button>` : ""}</div>` : ""}${activity.length ? `<details class="activity-panel"><summary>活动记录（${activity.length}）</summary><ol id="activity-log">${activity.map((entry) => `<li>${esc(entry)}</li>`).join("")}</ol></details>` : ""}${page === "settings" ? settingsHtml() : `<main class="workspace">${chatHtml(topic())}${drillHtml(topic())}</main>`}</div>`;
  root.querySelectorAll<HTMLElement>(".markdown-content").forEach((element) => {
    element.querySelectorAll<HTMLAnchorElement>("a[href]").forEach((link) => { if (/^(https?:|mailto:)/i.test(link.getAttribute("href") ?? "")) { link.target = "_blank"; link.rel = "noreferrer noopener"; } });
    renderMathInElement(element, { delimiters: [{ left: "$$", right: "$$", display: true }, { left: "$", right: "$", display: false }, { left: "\\[", right: "\\]", display: true }, { left: "\\(", right: "\\)", display: false }], ignoredTags: ["script", "noscript", "style", "textarea", "pre", "code"], throwOnError: false });
  });
}
async function call(command: string, args: Record<string, unknown> = {}, message = "正在处理…"): Promise<void> {
  if (busy) return;
  busy = true; cancellable = ["generate", "regenerate_exercises"].includes(command); cancelRequested = false; activity = [message]; error = ""; status = message; render();
  try {
    state = await invoke<AppState>(command, args);
    settings = structuredClone(state.settings);
    status = "操作完成";
    if (["new_topic", "switch_topic", "delete_topic", "generate", "follow_up"].includes(command)) chatDraft = "";
    if (["new_topic", "switch_topic", "select_exercise", "submit_answer", "regenerate_exercises"].includes(command)) answerDraft = "";
    if (["new_topic", "switch_topic", "select_exercise", "regenerate_exercises"].includes(command)) { experimentDraft = null; experimentLanguage = ""; }
  } catch (cause) { error = String(cause); status = ""; }
  finally { busy = false; cancellable = false; cancelRequested = false; render(); }
}
root.addEventListener("input", (event) => {
  const target = event.target;
  if (!(target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement || target instanceof HTMLSelectElement)) return;
  if (target.dataset.field === "chat") chatDraft = target.value;
  if (target.dataset.field === "answer") answerDraft = target.value;
  if (target.dataset.field === "experiment-code") experimentDraft = target.value;
  if (target.dataset.field === "experiment-language") experimentLanguage = target.value;
  if (target.dataset.field === "rename") renameDraft = target.value;
  const key = target.dataset.setting as keyof Settings | undefined;
  if (key && settings && key !== "available_models") (settings as unknown as Record<string, string>)[key] = target.value;
});
root.addEventListener("change", (event) => {
  const target = event.target;
  if (target instanceof HTMLSelectElement && target.dataset.field === "sort") void call("set_topic_sort", { sort: target.value }, "正在更新排序…");
});
root.addEventListener("keydown", (event) => {
  const target = event.target;
  if (!(target instanceof HTMLTextAreaElement) || !target.classList.contains("code-input") || event.key !== "Tab") return;
  event.preventDefault(); const start = target.selectionStart; const end = target.selectionEnd;
  target.value = target.value.slice(0, start) + "  " + target.value.slice(end);
  target.selectionStart = target.selectionEnd = start + 2; answerDraft = target.value;
});
root.addEventListener("click", (event) => {
  const target = event.target;
  if (!(target instanceof Element)) return;
  const externalLink = target.closest<HTMLAnchorElement>("a[href]");
  if (externalLink && /^(https?:|mailto:)/i.test(externalLink.href)) {
    event.preventDefault();
    void openUrl(externalLink.href).catch((cause: unknown) => { error = `打开链接失败：${String(cause)}`; render(); });
    return;
  }
  const button = target.closest<HTMLButtonElement>("button[data-action]");
  if (!button) return;
  const id = button.dataset.id;
  switch (button.dataset.action) {
    case "cancel-task": cancelRequested = true; status = "正在取消任务…"; render(); void invoke<boolean>("cancel_task").catch((cause: unknown) => { error = String(cause); render(); }); break;
    case "workspace": page = "workspace"; render(); break;
    case "settings": page = "settings"; render(); break;
    case "toggle-keys": showKeys = !showKeys; render(); break;
    case "new": void call("new_topic", {}, "正在新建话题…"); break;
    case "switch": if (id) void call("switch_topic", { topicId: id }, "正在切换话题…"); break;
    case "delete": if (id && window.confirm("删除这个话题及其练习记录？")) void call("delete_topic", { topicId: id }, "正在删除话题…"); break;
    case "rename-start": renameId = id ?? null; renameDraft = state?.topics.find((item) => item.id === id)?.title ?? ""; render(); break;
    case "rename-cancel": renameId = null; renameDraft = ""; render(); break;
    case "rename-save": if (renameId) { void call("rename_topic", { topicId: renameId, title: renameDraft }, "正在重命名…"); renameId = null; } break;
    case "select": if (id) void call("select_exercise", { exerciseId: id }, "正在切换练习…"); break;
    case "generate": void call("generate", { topicText: chatDraft }, "AI 正在讲解并生成练习…"); break;
    case "follow-up": void call("follow_up", { question: chatDraft }, "AI 正在回复追问…"); break;
    case "submit": void call("submit_answer", { answer: answerDraft }, "AI 正在评审答案…"); break;
    case "regenerate": void call("regenerate_exercises", {}, "AI 正在重新生成练习…"); break;
    case "experiment": void call("request_experiment", {}, "AI 正在生成实验方案…"); break;
    case "execute-experiment": void call("execute_experiment", { code: experimentDraft ?? exercise(topic())?.starter_code ?? "", language: experimentLanguage || topic()?.concept?.language || "py" }, "沙箱正在运行代码…"); break;
    case "save-settings": if (settings) void call("save_settings", { settings }, "正在保存设置…"); break;
    case "fetch-models": if (settings) void call("fetch_models", { settings }, "正在拉取模型列表…"); break;
  }
});
void listen<string>("task-progress", (event) => { status = event.payload; activity.push(status); activity = activity.slice(-30); const line = root?.querySelector(".status-line"); if (line && !error) { const label = line.firstChild; if (label) label.textContent = status; } const log = root?.querySelector("#activity-log"); if (log) log.innerHTML = activity.map((entry) => `<li>${esc(entry)}</li>`).join(""); });
void Promise.all([invoke<AppState>("get_state"), invoke<string>("get_storage_path"), invoke<typeof skills>("list_skills")]).then(([loaded, path, enabledSkills]) => { state = loaded; settings = structuredClone(loaded.settings); storagePath = path; skills = enabledSkills; status = loaded.status || "已加载本地历史记录"; render(); }).catch((cause: unknown) => { error = `启动失败：${String(cause)}`; status = ""; render(); });
render();
