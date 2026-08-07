@.claude/agents.md

## Tone & Response Rules

- No sycophancy. No praise, no "great question", no filler.
- Minimal responses. Answer directly, then stop.
- No trailing summaries unless asked.
- Fragments OK. Short synonyms preferred.

## Development Guidelines

- **Dependency Minimalist:** NEVER add a new dependency unless explicitly instructed by the user.
- **Alternative First:** Even if a new dependency is requested, always research and present alternative ways to implement the functionality using existing tools, native APIs, or lightweight manual implementations before proceeding.
- **Memory Management:** Prefer the stack over the heap whenever possible to maintain low RSS and high performance.
- **Minimalist Implementation:** When implementing changes or new features, always prioritize the minimal required code. Avoid sweeping architectural changes or over-engineering; keep the solution focused and surgical.
- **No Dead Code:** `#[allow(dead_code)]` is **NOT** a solution and is strictly prohibited. If code is unused, it must be removed. Do not introduce "pre-emptive" or "placeholder" infrastructure.
- **Terminal Restraint:** NEVER run any command-line commands in the terminal unless explicitly instructed to do so by the user during the current chat session.
- **Atomic Step Policy:** Execute precisely one change, command, or file edit per response. After each step: 1) Describe exactly what was changed, 2) Provide a specific verification method or command to test the change, 3) Stop and wait for user confirmation before proceeding. Never chain multiple logical tasks or batch edits unless told to do so by user.
- **Plan Step Journaling:** When executing a plan, after completing a step, append a completion note to that step in the plan document describing what was done and how. Do NOT alter the original step text — only add below it. Do this before moving to the next step.
- **Investigation Boundary:** When asked to investigate, research, or audit, NEVER modify any code. Document findings and analysis in a markdown file for easy reference during triage. Stand by for instructions after delivery.
- **Planning Neutrality:** When asked to create a plan, provide the logical step-by-step breakdown in a markdown file for easy reference during execution. NEVER modify code or execute commands while planning. Stand by for verification and selection of the first step.
- **Git Read-Only Policy:** Git commands are ONLY permitted for historical research and investigation (e.g., `git log`, `git show`, `git blame`). NEVER use git to commit, branch, stage changes, or otherwise manipulate the repository state.
- **Commit Baselining:** Once the user states they have committed changes, those changes are considered part of the project baseline. DO NOT reference these previously committed changes as "what has changed" or "pending modifications" in subsequent messages.
- **Surgical Refactoring:** When asked to refactor, only modify the specific code requested. Zero tolerance for functional regressions or unintended behavior changes; the refactor must be behaviorally identical to the original.
- **Functional Parity Guarantee:** Every refactor or optimization MUST maintain absolute 1:1 functional parity with the preceding code. If an optimization risks changing an edge case, it is NOT permitted without explicit user approval.
- **Edge Case Preservation:** Regressions are strictly prohibited. Assume that every existing edge case or "weird" check in the code is there for a critical reason. Before removing or modifying complex logic, verify its origin and purpose via `git blame` or research.
- **Conversational Boundaries:** When asked for "thoughts," "opinions," or "feedback," NO code changes or command executions are implied. Provide textual analysis and wait for explicit instructions before taking action.
- **Zero Polling Policy:** NO polling logic should ever be introduced. janq is event-driven; all monitoring must be implemented via signals, interrupts, or reactive primitives. If existing polling logic is encountered, notify the user of its presence and rationale.
- **Caching Over Discovery:** Avoid expensive, repetitive operations like looping through all open windows. Rely on internal window and PID cache whenever possible. Discovery/matching passes should only be triggered as a fallback when an entry is missing or invalid.

## Build & Cross-Compilation

- [build.rs](build.rs): On Windows targets, embeds the application icon (`icon.ico`) into the executable via [assets/janq.rc](assets/janq.rc) using the `embed-resource` crate.
- **Cross-compile targets:** `x86_64-pc-windows-gnu`, `x86_64-unknown-linux-musl`.
- **Release profile:** `opt-level = "z"`, LTO, single codegen unit, panic=abort, stripped — optimized for minimal binary size.
- No feature flags. The tray menu is always available on Linux via the native `zbus` dbusmenu implementation.

