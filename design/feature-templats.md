# User story templates that actually work for AI coding agents

**Spec-driven development has replaced ad-hoc prompting as the dominant paradigm for directing AI coding agents in 2025-2026.** The traditional "As a user, I want X, so that Y" story format is no longer sufficient — teams achieving the best results now use structured, machine-readable specifications with explicit context packages, verifiable acceptance criteria, and three-tier boundary systems. GitHub's analysis of **2,500+ agent configuration files** reveals that the most effective specifications cover six core areas: commands, testing, project structure, code style, git workflow, and boundaries. Meanwhile, research from Columbia University, METR, and OX Security demonstrates that vague or poorly structured stories produce silent failures, code soup, and — counterintuitively — can make experienced developers **19% slower** rather than faster.

The shift from "prompt engineering" to what Anthropic calls **"context engineering"** underpins every successful framework. Context is a finite resource with diminishing marginal returns. The winning approach treats user stories as carefully engineered context windows — small enough to avoid information loss, specific enough to prevent hallucination, and structured enough to enable automated verification.

---

## Seven frameworks now define how teams write stories for AI agents

Several distinct template formats have emerged, each targeting different workflow needs. The most significant are spec-driven development frameworks that break work into phased, gated artifacts rather than single-prompt stories.

**GitHub Spec Kit**, released in September 2024 and widely adopted through 2025, implements a four-phase workflow: Specify → Plan → Tasks → Implement. Each phase produces a markdown artifact (spec.md, plan.md, tasks.md) that serves as a contract between human and agent. Tasks include parallel-execution markers and user story labels for traceability. It supports **15+ agents** including Copilot, Claude Code, Gemini CLI, and Cursor, making it the closest thing to an industry standard.

**AWS's Kiro IDE** uses three specification files (requirements.md, design.md, tasks.md) with acceptance criteria written in **EARS format** (Easy Approach to Requirements Syntax): `WHEN the user selects a file THEN the system SHALL validate the format`. This syntax sits between natural language and Gherkin, providing machine-parseable structure without the ceremony of full BDD. Kiro reached general availability in November 2025 and reports compressing feature development from weeks to days.

**The Agent Stories framework** by Slava Kurilyak flips the traditional user story by making the AI agent the first-class actor: "As a Claude Code agent, I need access to the existing database schema, business requirements document, and data model standards to create a new user authentication table with proper relationships so that the backend team can integrate it immediately." Each story includes a context package (specific files, APIs, standards), verification commands (scriptable checks), integration paths, and explicit constraints. The rationale is mathematical: a **5% error rate per action** compounds to only 59.9% success over 10 turns, so breaking work into small, verifiable units is essential.

**ProdMoh's JSON-structured stories** take machine-readability furthest. Stories are expressed as JSON objects with predicate-based acceptance criteria (`response.status == 200`), concrete input/output examples, explicit constraints, and structured non-functional requirements (`latency_p95 <= 250ms`). These stories are consumed directly by AI agents through MCP integrations inside IDEs.

Other notable frameworks include **JetBrains Junie's SDD prompt** (a complete four-file template generating requirements, plan, tasks, and guidelines), the **BMAD Method** (12+ specialized AI agent personas cycling through Analysis → Planning → Solutioning → Implementation), and **Addy Osmani's spec framework** (six core areas distilled from GitHub's 2,500-repo analysis). All converge on the same insight: **specifications must be living, version-controlled documents** that evolve alongside the codebase.

---

## The six structural elements that separate effective stories from noise

GitHub's empirical analysis of thousands of agent configuration files, combined with practitioner reports from Microsoft, Anthropic, and Google, reveals consistent patterns in what makes specifications effective.

**Commands must be executable, not descriptive.** Writing `npm test` or `pytest -v` works; writing "run the tests" does not. Agents reference commands constantly, and ambiguity here propagates through every verification step. **Testing expectations** need to specify the framework, file locations, coverage thresholds, and sample test cases. **Project structure** must explicitly map directories to purposes — `src/` for application code, `tests/` for unit tests — because agents otherwise misplace files or create duplicate structures.

**Code style is best communicated through a real code snippet** rather than written rules. One concrete example anchors the agent to your patterns more effectively than paragraphs of description. **Git workflow** instructions (branch naming, commit format, PR requirements) are followed reliably when spelled out but ignored when assumed. And **boundaries** — the single most impactful element — work best as a three-tier system:

- **✅ Always**: Actions safe to take autonomously ("Run tests before commits")
- **⚠️ Ask first**: Actions requiring human approval ("Database schema changes," "Adding new dependencies")
- **🚫 Never**: Hard stops ("Commit secrets," "Edit node_modules/," "Remove a failing test without approval")

Research confirms a critical calibration challenge dubbed the **"curse of instructions."** As you pile on more requirements, model performance in adhering to each one drops significantly. Even GPT-4 and Claude struggle when asked to satisfy many requirements simultaneously — with 10 detailed rules, the AI typically obeys the first few and overlooks others. Microsoft's Developer Tools research group found that **prompts with explicit specifications reduced back-and-forth refinements by 68%**, but only when focused. The practical implication: decompose complex requirements into sequential, simple instructions rather than monolithic specifications.

---

## AGENTS.md and the new configuration file ecosystem

A new infrastructure layer has emerged between project management tools and AI coding agents: **repository-level configuration files** that provide persistent context across sessions.

The **AGENTS.md** open standard, stewarded by the Agentic AI Foundation under the Linux Foundation (with Google, OpenAI, Factory, Sourcegraph, and Cursor as founding members), has been adopted by **60,000+ open-source repositories**. It functions as "a README for agents" — a dedicated location for build steps, test commands, coding conventions, and architecture context. It supports hierarchical nesting so subdirectory files can override root-level instructions, similar to .gitignore. OpenAI's own repository contains **88 AGENTS.md files**.

Each major tool also maintains its own configuration format: **CLAUDE.md** for Claude Code (best kept under 300 lines, focused on what the agent would get wrong without it), **.github/copilot-instructions.md** for GitHub Copilot (with path-specific `*.instructions.md` files and custom chat modes), **.cursor/rules/*.mdc** for Cursor, **CONVENTIONS.md** for Aider, and tool-specific files for Windsurf, Cline, Roo Code, and others.

The fragmentation problem is solved by **Ruler**, an open-source CLI tool that maintains a single source of truth in `.ruler/*.md` files and distributes rules to **30+ agent configuration formats** with one command. This means teams write their conventions once and `ruler apply` propagates them to every AI tool in use.

On the project management side, **MCP (Model Context Protocol) servers** now bridge Jira, Linear, and GitHub Issues directly into AI coding environments. The DX Heroes MCP server connects AI assistants to Jira and Linear via natural language from within the IDE. Atlassian's official Rovo MCP server provides OAuth-secured access to Jira issues and Confluence pages. **GitHub Copilot for Jira** (both third-party and an official public preview) enables one-click assignment of Jira issues to Copilot's coding agent, which analyzes the ticket, generates code, and opens a PR. Port.io offers an orchestration layer that enriches Jira tickets with catalog context before routing them to coding agents.

---

## Nine critical failure patterns and why vague stories produce silent bugs

Columbia University's DAPLab iteratively built **15+ applications** using five leading coding agents and cataloged hundreds of failures into **nine distinct patterns**. The most insidious finding: AI agents prioritize runnable code over correct code, **suppressing errors silently** rather than surfacing them. Traditional software bugs cause visible crashes; AI-generated bugs produce code that "appears to work while embedding subtle vulnerabilities and technical debt."

The nine failure patterns are: UI grounding mismatch (agents cannot see interfaces), state management failures during refactoring, business logic misinterpretation (applying a discount rule to individual items instead of the cart), data model confusion (failing to understand schemas they generated themselves), API hallucination (fabricating environment variables rather than asking for real values), security vulnerability introduction, code duplication instead of abstraction, codebase awareness degradation as file count grows, and silent error suppression.

OX Security's analysis of **300+ repositories** found that **80-90% of AI-generated code** exhibits "refactoring avoidance" — implementing prompts directly without considering existing code structure. Context blindness produces duplicated logic and inconsistent naming across files. And **inflated unit test coverage** gives a false sense of safety: shallow tests prove code runs, not that it's correct.

The specification-level anti-patterns most consistently cited across practitioner reports are:

- **Vague prompts** that could apply to dozens of scenarios, producing generic output
- **Overloaded prompts** asking for authentication, a React frontend, and deployment scripts simultaneously — yielding jumbled, incomplete results
- **Missing negative constraints**, causing agents to pull in unnecessary libraries or use unsafe patterns
- **Context window bloat** from too many MCP servers or oversized instruction files
- **Mid-task scope changes**, which Cognition's own Devin performance review calls out: "It usually performs worse when you keep telling it more after it starts the task"
- **Sunk cost fallacy** — continuing a conversation where the agent has gone down the wrong path instead of clearing context and starting fresh

---

## Case studies reveal a stark divide between structured and unstructured approaches

The most instructive case study contrast comes from Devin. Answer.AI's rigorous one-month test produced **14 failures, 3 successes, and 3 inconclusive results** across 20 tasks. One team member described the output as "spaghetti code that was way more confusing to read through than if I'd written it from scratch." Tasks the agent could handle "are those that are so small and well-defined that I may as well do them myself." When asked to deploy to a platform with limitations it didn't understand, Devin spent over a day attempting approaches and **hallucinating features that didn't exist**.

Yet Cognition's own annual review shows Devin's PR merge rate doubling from **34% to 67%** when teams provide clear specifications. At Nubank, migration tasks ran **8-12x faster** with structured requirements — each file migration completing in 3-4 hours versus 30-40 human hours. Security vulnerability resolution achieved **20x efficiency gains**. The variable is specification quality, not tool capability.

Boris Cherny, creator of Claude Code, demonstrates the plan-first workflow that works at Anthropic: "I use Plan mode and go back and forth with Claude until I like its plan. From there, I switch into auto-accept mode and Claude can usually one-shot it. **A good plan is really important.**" His team's CLAUDE.md is a living document — "Anytime we see Claude do something incorrectly we add it, so Claude knows not to do it next time." Every mistake becomes a rule, creating a self-improving specification loop. He runs **5 local sessions and 5-10 remote sessions simultaneously**, each on its own git checkout, demonstrating the parallel orchestration that structured specs enable.

CodeScene's AI team reports a **2-3x speedup** after going fully agentic — but only with objective quality measurement through Code Health scores, MCP safeguards, and mandatory AGENTS.md files. One team that created an AI prompting playbook with examples of good and bad prompts saw **code quality improve by approximately 60%**.

---

## A practical template for teams starting today

Synthesizing across all frameworks and evidence, the minimum viable story format for AI coding agents needs these components:

```markdown
## [Clear, descriptive title]

### Context
- Tech stack: [Specific frameworks and versions]
- Relevant files: [Paths to files the agent should read or modify]
- Existing patterns: [Link to or describe conventions to follow]
- Architecture: [Key decisions and constraints]

### Task
[Single clear goal — what to build and why it matters]

### Acceptance criteria
- [ ] [Specific, verifiable outcome with concrete values]
- [ ] [Edge case handling expectations]
- [ ] Tests: [Framework, location, what scenarios to cover]
- [ ] Verification: `[executable command to confirm completion]`

### Examples
- Input: [concrete input] → Expected output: [concrete output]

### Boundaries
- ✅ Always: [Safe defaults — run tests, follow naming conventions]
- ⚠️ Ask first: [Schema changes, new dependencies, CI/CD modifications]
- 🚫 Never: [Secrets in code, removing failing tests, modifying vendor files]
```

The key principles behind this template: **spec first, code second** (validate the plan before implementation); **one task per prompt** (modular over monolithic); **commands, not descriptions** (`pytest -v` not "run the tests"); **code examples over explanations** (show the pattern you want); **explicit versions and constraints** (leave nothing to inference); and **human checkpoints** (verify at phase boundaries before proceeding).

## Conclusion

The evidence converges on a clear thesis: **the quality of AI coding output is bounded by the quality of the specification, not the capability of the model.** Teams treating AI agents like senior developers who "just figure it out" consistently produce worse results than teams treating them like capable but literal junior engineers who need precise context, clear boundaries, and verifiable checkpoints. The most successful organizations have shifted from writing stories for human interpretation to engineering context for machine consumption — adding executable commands, predicate-based acceptance criteria, concrete examples, and three-tier boundaries while keeping each specification focused enough to avoid the curse of instructions. The tooling ecosystem (AGENTS.md, Ruler, MCP servers, Jira integrations) has matured enough that this structured approach now integrates seamlessly into existing agile workflows rather than replacing them.
