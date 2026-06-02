---
name: skill-creator
description: Use when creating or updating Pure project skills. Guides SKILL.md structure, frontmatter, support directories, project-level writes, and self-learning boundaries.
category: skills
---

# Pure Skill Creator

Use this skill when the user asks to create, update, review, or improve a Pure skill.

## Skill Shape

Every skill is a directory containing `SKILL.md`.

The file must start with YAML frontmatter:

```markdown
---
name: example-skill
description: Use when ...
category: optional-category
platforms: ["windows", "linux", "macos"]
---
```

`name` and `description` are required. The description should say when the skill should trigger, not merely what the skill is.

## Project Writes

Pure self-learning and `skill_manage` write only to the current project skills directory:

```text
<workspace_root>/skills/
```

System, user, and external skills are read-only. To customize one, create a project skill with the same name or create a project-specific companion skill.

## Supported Files

Keep `SKILL.md` concise. Put additional context in:

- `references/` for documentation loaded only when needed.
- `templates/` for reusable text or code templates.
- `scripts/` for deterministic helper scripts.
- `assets/` for files reused in outputs.

Do not create extra README, changelog, quick-reference, or process notes unless they are directly used by the skill.

## Editing Workflow

1. Use `skills_list` to inspect existing skills.
2. Use `skill_view` before editing a relevant existing skill.
3. Prefer `skill_manage` patch for small updates.
4. Use full edit only when the skill needs substantial restructuring.
5. Validate that frontmatter still has the same target name after any patch/edit.

## Quality Bar

- Keep instructions operational and short.
- Store project-specific lessons in project skills, not in system/user skills.
- Do not capture one-off tasks, transient provider/tool failures, or private user preferences.
- Mention related support files from `SKILL.md` so future agents know what to load.
