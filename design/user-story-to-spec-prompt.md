# User Story to Coding Agent Specification Prompt

Based on 2024-2025 research showing persona prompts ("you are an expert") provide ~0% improvement while specification-based approaches deliver 35-80% gains.

---

## The Prompt

````markdown
You are preparing a user story for handoff to an AI coding agent. You have access to the codebase context provided below.

## Your Task

Transform the user story outline into a complete implementation specification that a coding agent can execute without ambiguity.

## Input

<user_story>
{{USER_STORY_OUTLINE}}
</user_story>

<codebase_context>
{{RELEVANT_FILES_TYPES_CONVENTIONS}}
</codebase_context>

## Required Analysis (complete before writing specification)

1. **Clarify ambiguities**: List any assumptions you're making where the user story is underspecified
2. **Identify affected components**: Which files/modules will be created or modified?
3. **Map data flow**: What inputs enter the system, how are they transformed, what outputs are produced?
4. **Surface edge cases**: What happens with empty inputs, invalid data, concurrent access, network failures?
5. **Determine integration points**: What existing code does this touch? What contracts must be preserved?

## Output Specification Format

### Summary

One sentence describing what this implementation accomplishes.

### Acceptance Criteria

Concrete, testable conditions that define "done". Format as:

- GIVEN [precondition] WHEN [action] THEN [observable result]

### Technical Approach

Brief description of the implementation strategy and rationale for key decisions.

### Function Signatures

\```{{LANGUAGE}}
// For each new function or modified signature:
// - Full type annotations
// - Parameter descriptions
// - Return type and meaning
// - Throws/errors that callers must handle

function_name(param1: Type, param2: Type) -> ReturnType
\```

### Implementation Breakdown

Ordered list of discrete implementation steps. Each step should be:

- Small enough to verify independently
- Named with intent (what it accomplishes, not how)
- Include expected file location

### Edge Cases & Error Handling

For each edge case identified:

- Condition that triggers it
- Expected behavior
- How to verify correct handling

### Test Cases

Concrete test scenarios:
\```
TEST: [descriptive name]
SETUP: [preconditions]
ACTION: [what is invoked]
ASSERT: [expected outcome]
\```

### Dependencies & Prerequisites

- External packages needed
- Environment configuration
- Database migrations or schema changes
- Other stories that must be completed first

### Files to Modify/Create

| File Path    | Action        | Purpose           |
| ------------ | ------------- | ----------------- |
| path/to/file | CREATE/MODIFY | Brief description |

### Out of Scope

Explicitly list related functionality that is NOT part of this story to prevent scope creep.

---

## Quality Checks Before Finalizing

Verify your specification:

- [ ] Every acceptance criterion is testable by a machine
- [ ] Function signatures include all type information
- [ ] Edge cases cover: empty/null inputs, invalid types, boundary values, failure modes
- [ ] Implementation steps are ordered by dependency (earlier steps don't depend on later ones)
- [ ] A coding agent reading only this spec has everything needed to implement and verify
````

---

## Design Rationale

| Decision                              | Research Basis                                                     |
| ------------------------------------- | ------------------------------------------------------------------ |
| No "you are an expert" framing        | Persona prompts show ~0% improvement, sometimes degradation        |
| Explicit analysis phase before output | Self-planning showed +25.4% improvement over direct generation     |
| Function signatures with full types   | +10-12 percentage points in CoderEval studies                      |
| Structured output sections            | SCoT's structural approach showed +13.79% on HumanEval             |
| Test cases with SETUP/ACTION/ASSERT   | Feedback loop integration (Reflexion achieved 91% vs 80% baseline) |
| Checklist verification at end         | Aligns with "persistence" instruction pattern (+20% on SWE-bench)  |
| "Out of Scope" section                | Devin's "defensive prompting"—anticipate confusion points          |
| Markdown tables for files             | Familiar format (Aider research on format alignment)               |

---

## Optional: Multi-Agent Orchestration Extension

For systems like Legion that assign work to multiple agents:

```markdown
### Agent Assignment Hints

- Complexity estimate: [LOW/MEDIUM/HIGH]
- Parallelizable steps: [list steps that have no interdependencies]
- Human review gates: [steps requiring human approval before proceeding]
- Estimated token budget: [rough context size the implementing agent will need]
```

---

## Key Research Sources

- **Persona ineffectiveness**: "When 'A Helpful Assistant' Is Not Really Helpful" (2024) - 162 personas, 4 LLM families, minimal effect
- **Self-planning**: Jiang et al. 2024 - +25.4% Pass@1 improvement
- **Structured CoT**: Li et al. 2024 - +13.79% on HumanEval
- **Specification-based prompting (µFiX)**: +35-80% Pass@1 improvement
- **OpenAI GPT-4.1 Guide**: Three-part system prompts +20% on SWE-bench
- **AlphaCodium**: Flow engineering improved GPT-4 CodeContests from 19% to 44%
- **Anthropic Claude Code Best Practices**: Context engineering over persona assignment
- **Cognition Devin**: "Say HOW you want things done, not just what"
