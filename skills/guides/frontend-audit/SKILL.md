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
- **Tauri invoke commands**: `run_prompt`, `stop_prompt`, `implement_plan`, `approve_tool`, `answer_user_input`, etc.

## Error Recovery

- If `apply_patch` fails during migration: read the current file with `read_file`, then submit a smaller patch
- After each phase: run `npm run build` (or `tsc --noEmit && vite build`) to verify no TS/build errors
- Do not modify `state/`, `hooks/`, `lib/` business logic during a pure UI migration
- Do not modify `src-tauri/` (Rust backend) during frontend refactoring
