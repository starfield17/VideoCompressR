# AGENTS.md

1. State assumptions before coding.
2. Prefer the simplest compatibility-preserving change.
3. Do not add unrequested flexibility.
4. Touch only files allowed by the task.
5. Do not clean unrelated code.
6. Define tests before declaring success.
7. Do not change public contracts silently.
8. Stop and emit a Decision Request when contracts are ambiguous.
9. Every changed line must trace to the active Goal or acceptance criteria.
10. No completion claim without actual command evidence.

`reffer/` is read-only reference material. Root builds, tests, and runtime paths
must not depend on it. Rust business rules belong in `vc-core`; external tools,
filesystem, queues, and application services belong in `vc-runtime`; CLI and
Tauri are thin adapters.
