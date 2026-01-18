# Multi-Agent Workflow

## Core Principle: One Owner, Two Reviewers

For any deliverable (proposal or code), one agent takes ownership, the other two review.

## Agent Roles

| Agent | Primary Ownership | Review Focus |
|-------|-------------------|--------------|
| **Gemini** | High-level architecture design | Business logic, data structure correctness |
| **Claude Code** | Business flow, logic, data structures, code implementation | Architecture alignment, edge cases |
| **Codex** | Complex problem solving, debugging | Architecture, logic, code quality |

**Constraints:**
- Gemini: web prototypes (HTML/CSS/JS) only for code

---

## Context Retrieval (Available at Any Phase)

**Tool**: Augment Context Engine (`mcp__ace-tool__search_context`)

Use whenever context is needed:
1. Natural-language semantic queries (What/Where/How)
2. Do not answer based on assumptions
3. Recursive retrieval until context is complete
4. Ask clarifying questions if requirements remain ambiguous

**Forbidden**: `grep`/`rg` keyword search for initial context gathering.

---

## Phase 1: Analysis & Planning

**Tools**: Codex AND Gemini (dual-model)

**Step 1: Workload Assessment**
- Claude Code evaluates task complexity and scope
- Small/simple tasks: ask the user to confirm whether skip Step 2 to proceed directly to implementation
- Large/complex tasks: proceed to Step 2

**Step 2: Proposal Draft**

| Aspect | Owner | Reviewers |
|--------|-------|-----------|
| Architecture design | Gemini | Claude Code, Codex |
| Business flow & logic | Claude Code | Gemini, Codex |
| Data structures | Claude Code | Gemini, Codex |

**Process:**
1. Gemini drafts architecture proposal → Claude Code & Codex review
2. Claude Code drafts business logic & data structures → Gemini & Codex review
3. Write consolidated proposal to `.claude/drafts` directory

**Focus on (high priority):**
- Component relationships and boundaries
- Business flow and logic
- Data structures and models
- Interface contracts between modules
- Affected files and potential risks

**Avoid (low priority):**
- Detailed code implementation
- Syntax or compilation concerns
- Language-specific idioms
- Error handling details

**Pseudocode**: minimal, only for clarifying complex algorithms or flows

**Step 3: Cross Review Loop**
1. Each aspect reviewed by the other two agents
2. Gemini validates architecture; Codex & Claude Code review logic
3. Claude Code & Codex validate business logic; Gemini reviews architecture alignment
4. Synthesize feedback, revise proposal
5. **Repeat until consensus reached among all three agents**

**Step 4: User Confirmation**
- Present final proposal to user
- Proceed only after user approval

## Phase 2: Prototyping & Implementation

**Ownership Model:**

| Deliverable | Owner | Primary Reviewer | Final Reviewer |
|-------------|-------|------------------|----------------|
| Web prototypes (HTML/CSS/JS) | Gemini | Claude Code | Codex |
| All other code | Claude Code | Codex | Gemini |

**Non-Web Code Review Loop:**
```
┌─────────────────────────────────────────────────────┐
│  Claude Code writes code                            │
│         ↓                                           │
│  Codex reviews → issues found? ──yes──→ fix & loop  │
│         ↓ no                                        │
│  Gemini final review → issues found?                │
│         ↓ no                     ↓ yes              │
│  Review passed         Claude Code + Codex evaluate │
│         ↓                            ↓              │
│  Static analysis              fix needed? ─yes→loop │
│  (clang-tidy, clazy)               ↓ no             │
│         ↓                    document & proceed     │
│  Format code                                        │
│  (clang-format)                                     │
│         ↓                                           │
│      Complete                                       │
└─────────────────────────────────────────────────────┘
```

**Workflow:**
- Codex output is reference only, Claude Code makes final implementation
- Request `Unified Diff Patch` output only from external agents

**Implementation Standards:**
1. Refactor prototype logic into production-grade code
2. No comments/docs unless necessary
3. Changes must stay within requirement boundaries
4. Review for side effects, fix any regressions

**Testing Requirements (for Rust, Go, Python, Node.js):**
- New code: add corresponding test cases
- Modified code: add new test cases; only modify existing tests if business logic changed
- Run all tests after changes, ensure 100% pass before proceeding

**Static Analysis & Formatting (for C++/Qt):**

Execute **after code review passes**, in strict order:

1. **Static Analysis** - run both tools, fix all issues:
   - `clang-tidy -p cmake-build <source_files>` (config: `.clang-tidy`)
   - `clazy-standalone -p cmake-build <source_files>` (config: `.clazy`)
   - `cargo test`
   - `cargo clippy`
   - `go test`

2. **Code Formatting** - apply after static analysis passes:
   - C++: `clang-format -i <source_files>`
   - Rust: `cargo fmt`
   - Go: `go fmt <source_files>`
   - QML: `qmlformat -i <qml_files>`

All checks must pass before delivery.

## Phase 3: Delivery

**Pre-delivery Checks:**
- Run full test suite (for Rust, Go, Python, Node.js)
- All tests must pass before delivery
- Apply source code format

**Delivery:**
- Only deliver to user after review loop completes with no issues
- Include summary of changes and review iterations if relevant

## Critical Principles

- Never blindly trust Codex/Gemini - think independently
- Prototypes are logical reference only; rewrite for production
- Targeted changes only - never affect existing functionality
