# Rust engineering principles

## Timeless constraints — not a checklist

Choose the design that minimizes expected lifetime cost in THIS codebase:
implementation, review, learning, operation, debugging, compatibility, and future
change. Count costs imposed on callers and operators, not just the edited module.
Correctness, security, data integrity, explicit product contracts, and applicable
repository rules are constraints on that choice, not negotiable style preferences.

These principles complement the repository's AGENTS.md; they do not replace its
architecture, toolchain, safety, workflow, or release rules. Apply judgment and
briefly explain material trade-offs. Do not manufacture work to satisfy slogans.

## Refactor first — when structure obstructs the change

Before changing behavior, establish relevant tests and identify the responsibility
or dependency boundary involved. If the current structure would force a workaround,
duplicate authoritative knowledge, or cross that boundary, first perform the
smallest behavior-preserving refactor that enables the requested change. Verify it,
then change behavior and verify the result.

Keep structural and behavioral changes distinguishable. Do not mix unrelated
cleanup into the task. A change needing no preparatory refactor requires none.
Do not disable tests or alter expectations to disguise a behavioral change as a
refactor. An urgent incident correction may precede structural improvement when
that is the safer intervention; document the exception and remaining debt.

## 1. Cohesive responsibilities

Combine Separation of Concerns, Single Responsibility, and high cohesion/loose
coupling: group code by the knowledge it owns and the reasons it changes. Separate
policy, orchestration, presentation, persistence, and external integration where
their responsibilities differ. Organize coherent features internally; do not create
one-function modules or new crates merely to imitate a layer diagram. Preserve
existing crate boundaries and enforceable repository constraints.

## 2. Encapsulation and minimal contracts

Hide representation and mutable invariants behind the smallest useful public API.
Apply Interface Segregation: clients depend only on capabilities they need.
Separate reading, mutation, and administration when their consumers or authority
differ. Avoid exposing locks, storage handles, framework types, and internal
navigation paths across boundaries. Public fields remain appropriate for deliberate
passive data contracts. Law of Demeter concerns dependency knowledge, not counting
dots in fluent or iterator expressions.

## 3. Dependency inversion at real boundaries

Keep high-level policy independent of volatile I/O, frameworks, and implementation
details. Define contracts in terms of consumer/domain needs, not a vendor's API;
wire concrete implementations at the application boundary. Use existing modules,
functions, structs, and standard traits before inventing abstractions. A custom
trait is justified by a real boundary or substitution need, even with one production
implementation; it is not mandatory for every struct. Do not introduce service
locators or dependency-injection frameworks by default.

## 4. Behavioral substitutability

Apply Liskov Substitution to traits, adapters, and test doubles: an implementation
must satisfy the advertised contract, not merely compile. Do not strengthen
preconditions or weaken guarantees. Define error, ordering, cancellation,
idempotency, and durability semantics where callers rely on them. Test shared
contracts across implementations. A fake may implement a narrower capability;
it must not pretend to prove guarantees it cannot provide.

## 5. One authority per piece of knowledge

Apply DRY to business rules, identity, schemas, protocol semantics, and other
knowledge that must remain consistent. Reuse authoritative services; generate
derived contracts where the project supports generation. Do not edit generated
outputs independently. Separate representations for wire data, domain state, and
presentation may be necessary; keep their mapping explicit rather than forcing
one universal type. Similar-looking code is not automatically the same knowledge.
Caches and projections need explicit ownership, freshness, and invalidation rules.

## 6. Simplicity and evidence-based extension

Combine KISS, YAGNI, and disciplined Open/Closed: solve current requirements with
the fewest interacting concepts, not merely the fewest lines. Reuse stable extension
points before modifying their consumers. Add new extension points for observed
variation, an explicit requirement, or a demonstrated isolation/testing need—not
imagined futures. A second occurrence is evidence, not a numeric law. Keep closed
alternatives as enums when appropriate; do not replace an intentional data-driven
metamodel with a fixed enum catalogue. No speculative plugin systems or framework
layers. YAGNI does not excuse missing tests, recovery, or required security.

## 7. Composition and researched pattern selection

For non-trivial design or refactoring, inspect relevant existing code and tests,
research applicable Rust idioms/design patterns using primary documentation, and
consider relevant SOLID constraints. Compare the existing approach and the simplest
no-new-pattern alternative. Use a pattern only when it reduces a named source of
complexity or risk; briefly record the reason and trade-off in existing task notes
or documentation. Routine edits need no research ceremony.

Prefer composition through structs, functions, closures, enums, and narrow traits.
Use generics for useful compile-time variation and trait objects for useful runtime
variation; neither is a universal default. Adapt patterns to ownership and types,
not class-for-class translations. Avoid inheritance emulation, universal managers,
and generic frameworks built merely to satisfy a pattern name.

## 8. Explicit invariants, state, and failure

Use newtypes, enums, validated constructors, and typestate when they make important
mistakes harder without obscuring the design. Enforce dynamic rules at runtime
against their authoritative schema or metamodel. Validate at trust boundaries,
including deserialization; static types do not establish authorization or freshness
of external state. Use typed Result errors for failure and Option for genuine
absence. Reject malformed input without treating ordinary external failures as
reasons to crash. Never silently turn failure into successful empty/default output.
Follow the repository's panic and unsafe policies.

## 9. Ownership and bounded execution

Make state ownership, mutation authority, and task lifetime explicit. Prefer scoped
ownership; use shared locks or message passing according to the actual resource.
Do not use cloning, shared mutability, or unsafe code merely to evade a design
problem. Keep blocking lock guards away from await points. Define bounds for queues,
concurrency, retries, and input sizes, together with overload behavior. Make timeout,
cancellation, partial work, cleanup, and shutdown semantics deliberate. Destruction
alone is not proof of durable commit or completed asynchronous shutdown.

## 10. Evidence, reversibility, and deletion

Make behavior independently testable and important failures diagnosable. Protect
relevant existing behavior before refactoring; demonstrate a defect regression
before and after its fix when feasible. Use contract, integration, restart/recovery,
property, fuzz, or reference-conformance tests according to risk. A passing mock
is not evidence of interoperability or persistence. Measure performance claims.
Prefer localized, reviewable changes and removable components. Remove superseded
paths when safe; do not accumulate parallel implementations or compatibility shims
without an actual consumer and a removal condition. Report what was verified and
what remains unverified.

## Pattern selection guide — options, not required architecture

| Pattern or idiom | Use when | Avoid turning it into |
| --- | --- | --- |
| Ports and adapters / Adapter | Policy must be insulated from storage, transport, or external APIs. | A new trait and crate around every function. |
| Functional core, imperative shell | Decisions can operate on explicit inputs separately from I/O and orchestration. | A second competing domain model or blanket ban on state. |
| Strategy | A policy or algorithm genuinely varies. A closure may be sufficient. | A hierarchy or plugin registry for one fixed operation. |
| State machine / typestate | Lifecycle transitions or allowed operations need explicit enforcement. | Dozens of type parameters for dynamic, data-defined states. |
| Command | An operation needs to be represented as data for validation, approval, dispatch, or audit. | An automatic commitment to CQRS, event sourcing, or a command bus. |
| Newtype / Builder | Semantic distinctions or genuinely complex construction need support. | A wrapper around every primitive or a builder for every small struct. |
| Owning worker with bounded messages | An asynchronous resource benefits from one coordinating owner. | An actor framework for simple in-process data. |

## Project application notes

These notes reflect the root AGENTS.md files inspected on 30 September 2026,
not a complete source-code audit. Re-read the live policies before implementation.

**Dafrion Core:** preserve shared services behind HTTP/MCP/CLI, generic-engine versus
metamodel separation, exact identity, authoritative versus derived information,
and the existing governed change lifecycle. Do not hard-code DAF/ERF/R3AKT catalogues
or create a second persistence/mutation path. Keep generated contracts authoritative.
Static wrappers cannot replace runtime metamodel validation or exact-plan approval.
The root policy rejects arbitrary LOC caps and requires local acceptance evidence.

**LXMF-rs:** preserve declared workspace boundaries and library/application
separation. Judge parity by reference behavior, not method-name coverage. Protect
wire semantics and use shared contract/conformance tests where appropriate.
Existing policy includes module-size limits and architecture/release scripts;
these remain applicable until explicitly changed. Do not copy Dafrion-specific
workflow or toolchain assumptions into this repository.

## Research basis

The normative wording above is a project-oriented synthesis, not a quotation of
these sources. Sources were consulted on 30 September 2026.

- Robert C. Martin, *The Single Responsibility Principle* (2014):
  `https://blog.cleancoder.com/uncle-bob/2014/05/08/SingleReponsibilityPrinciple.html`
- Robert C. Martin, *Solid Relevance* (2020):
  `https://blog.cleancoder.com/uncle-bob/2020/10/18/Solid-Relevance.html`
- Martin Fowler, *An example of preparatory refactoring* (2015):
  `https://martinfowler.com/articles/preparatory-refactoring-example.html`
- Martin Fowler, *Yagni* (2015):
  `https://martinfowler.com/bliki/Yagni.html`
- *The Rust Programming Language*, trait objects and object-oriented patterns:
  `https://doc.rust-lang.org/book/ch18-02-trait-objects.html`
  `https://doc.rust-lang.org/book/ch18-03-oo-design-patterns.html`
- *Rust API Guidelines*, type safety and future proofing:
  `https://rust-lang.github.io/api-guidelines/type-safety.html`
  `https://rust-lang.github.io/api-guidelines/future-proofing.html`
- Alistair Cockburn, *Hexagonal architecture* (2005):
  `https://alistair.cockburn.us/hexagonal-architecture`
- *Rust Design Patterns*, community-maintained Strategy and Command examples:
  `https://rust-unofficial.github.io/patterns/patterns/behavioural/strategy.html`
  `https://rust-unofficial.github.io/patterns/patterns/behavioural/command.html`
- Tokio maintainers, shared state, channels, and graceful shutdown:
  `https://tokio.rs/tokio/tutorial/shared-state`
  `https://tokio.rs/tokio/tutorial/channels`
  `https://tokio.rs/tokio/topics/shutdown`
- Project policies inspected through the connected GitHub account:
  `https://api.github.com/repos/FreeTAKTeam/Dafrion/contents/AGENTS.md`
  `https://api.github.com/repos/FreeTAKTeam/LXMF-rs/contents/AGENTS.md`
