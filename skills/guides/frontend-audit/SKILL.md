---
name: frontend-audit
description: Use when auditing or improving Pure Studio's React frontend codebase. Covers parallel explorer agent layout, CSS architecture evaluation, design token audit, and large-component refactoring triggers.
category: guides
platforms: ["windows", "linux", "macos"]
---

# Pure Studio Frontend Codebase Audit

Trigger when the task involves evaluating the React frontend (`code/pure-studio/src/`) for refactoring, component library migration, theming, or performance improvements.

## Audit Methodology: Three-Pronged Explorer

Use 3 parallel `spawn_agent` (type: `explorer`) to cover the frontend codebase comprehensively:

| Agent | Scope | Key Files |
|-------|-------|-----------|
| **Styles** | CSS architecture, variables, color scheme, typography, spacing, responsive design | `styles/*.css`, `styles.css` |
| **Core UI** | Main layout, chat panel, timeline, project rail, context panel, status bar, approval overlay | `App.tsx`, `components/Conversation*.tsx`, `ProjectRail.tsx`, `ContextPanel.tsx`, `SessionStatusBar.tsx`, `ApprovalOverlay.tsx` |
| **Settings UI** | All settings tab pages and their form/edit patterns | `components/SettingsPage.tsx`, `Provider*.tsx`, `McpSettings.tsx`, `RoleSettings.tsx`, `SecuritySettings.tsx`, `SkillsSettings.tsx` |

Each agent must read full file contents (not just list). Parent session synthesizes findings.

## CSS Architecture Evaluation Checklist

When the Styles agent returns, evaluate these dimensions:

### 1. Design Tokens (CSS Variables)
- Are `--*` custom properties defined in `:root`?
- Are colors, spacing, border-radius, shadows hardcoded instead?
- Count distinct near-identical values (e.g., 5 different `#e2e8f0`-like grays)

### 2. Color Scheme & Theming
- Does the app support dark mode?
- Is there a mixed theme (e.g., dark sidebar + light content)?
- Are semantic colors consistent across files?

### 3. Typography
- Is there a defined type scale (font-size/weight hierarchy)?
- Are sizes hardcoded per component?

### 4. Spacing System
- Is there a base spacing unit (4px, 8px)?
- Do values follow multiples of the base unit?

### 5. Component Style Duplication
- Do semantically identical elements (cards, buttons, badges) repeat styling across CSS files?
- Could `.card`, `.btn`, `.badge` base classes be extracted?

### 6. Shadow & Border Consistency
- How many distinct `box-shadow` definitions exist?
- Are `border-radius` values semantically consistent?

## Large Component Detection

Pure-Lang enforces a ~500-line module limit (see CLAUDE.md). When evaluating frontend TSX components:

**Check `wc -l code/pure-studio/src/components/*.tsx`** and flag any file exceeding 600 lines. Common oversized targets:

| File | Typical Risk |
|------|-------------|
| `ConversationPanel.tsx` (~1182 lines) | Has inline timeline entry renderers (8+ entry types) |
| `SessionStatusBar.tsx` (~1226 lines) | Has hand-written Dropdown, Popover, Command-list logic |
| `ProviderSettings.tsx` (~735 lines) | Embeds draft editor overlay inline |

Each oversize component should be split:
- Extract inline sub-renderers into standalone components
- Replace hand-written float/dropdown/overlay primitives with shadcn/ui (or chosen library) equivalents
- Move form/fragment state into the caller or a dedicated sub-component

## Migration Prerequisites

Before any component library migration:

1. **First unify design tokens** — Define CSS custom properties (`:root` block) for all colors, spacing, radius, shadows. This prevents rewriting hardcoded values twice.
2. **Then migrate components** — Replace hand-written HTML+CSS with library components, referencing the token variables.
3. **Delete old CSS files** — Only after component migration and visual verification.

## Project-Specific Architecture Notes

- **State management**: Reducer-based (see `state/studio-state.ts`), Tauri events dispatch actions
- **Icons**: Always `lucide-react` (shadcn/ui's default icon set)
- **Virtual scroll**: Uses `@tanstack/react-virtual` for `ConversationTimeline`
- **i18n**: All text uses `useTranslation()` from `react-i18next`
- **No CSS-in-JS**: All styling is external CSS files with `className` — no styled-components or inline styles (except virtual scroll `transform` and Dropdown dynamic positioning)
- **Tauri event pattern**: Backend pushes via `listen("session_runtime")`, `listen("timeline_delta")`, `listen("tool_approval")`, `listen("user_input_request")`
- **Tauri invoke commands**: `run_prompt`, `stop_prompt`, `resolve_interaction`, etc.

## Constrained-Width List Item Layout Patterns

Pure Studio 侧边栏 (`w-60`, 240px) 中的会话/项目列表项容易因内容过多而布局异常。排查此类问题时检查以下典型原因：

### 1. 原始数值类型直接渲染

**典型症状**：标题显示不全，列表项右侧出现长串数字。

**原因**：`SessionRecord.updatedAt` 是 Unix 秒 `number`（如 `1779688800`），直接 `{session.updatedAt}` 渲染为 10 位数字，抢占标题空间。

**修复方案**：
- 如果时间信息在列表项中非必需（侧边栏已有排序），直接移除渲染
- 如果必须显示，格式化为短文本（如 `"3分钟前"`, `"2h"`），不要显示原始时间戳

### 2. Hover 动作按钮的布局稳定性

**典型症状**：鼠标移入/移出列表项时，标题宽度跳动，文字重新换行。

**原因**：hover 时显示的删除/操作按钮使用 `opacity-0/100` 切换可见性。虽然 `opacity: 0` 元素仍占布局空间，但在 Tailwind 某些组合（如同时使用 `transition-opacity` + `group-hover` + 动态 class 拼接）下可能触发浏览器重排。

**修复方案**：改用 `invisible`/`visible`（CSS `visibility`），确保按钮始终占据固定尺寸空间：

```tsx
// ❌ 可能引起跳动
className={`opacity-0 group-hover:opacity-100 ${selected ? "opacity-100" : "opacity-0"}`}

// ✅ 固定占位不跳动
className={`invisible group-hover:visible ${selected ? "" : "invisible"}`}
```

按钮保持固定尺寸（如 `w-7 h-7 shrink-0`），不要用 `w-auto` 或未显式设置宽度的方式。

### 3. `truncate` 生效条件

**典型症状**：明明加了 `truncate` class，标题仍然溢出换行。

**原因**：Tailwind 的 `truncate` 等价于 `overflow: hidden; text-overflow: ellipsis; white-space: nowrap;`。在 flex 容器中，还需要 `min-w-0`（覆盖 flex 默认的 `min-width: auto`）才能正确截断。

**确保截断的模式**：
```tsx
// flex 容器
<div className="flex items-center">
  {/* 标题：flex-1 占满剩余空间，min-w-0 允许收缩截断 */}
  <span className="flex-1 min-w-0 truncate">{title}</span>
  {/* 其他固定宽度的 flex-shrink-0 元素 */}
  <Button className="w-7 h-7 shrink-0" />
</div>
```

如果同一行有多个 `flex-shrink-0` 子元素（如时间标签+图标+按钮），确保它们都被标记为 `flex-shrink-0`，且标题是唯一 `flex-1` 的项。

## Error Recovery

- If `apply_patch` fails during migration: read the current file with `read_file`, then submit a smaller patch
- After each phase: run `npm run build` (or `tsc --noEmit && vite build`) to verify no TS/build errors
- Do not modify `state/`, `hooks/`, `lib/` business logic during a pure UI migration
- Do not modify `src-tauri/` (Rust backend) during frontend refactoring
