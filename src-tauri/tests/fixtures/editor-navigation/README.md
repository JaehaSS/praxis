# Editor navigation fixtures

Each fixture defines `target` and calls it once from `callerA` and `callerB`
(`caller_a` and `caller_b` in Rust/Python). Each fixture has a distinct
same-named `target` in another scope or file to test semantic isolation.

| Fixture | Target declaration | callerA call | callerB call |
| --- | --- | --- | --- |
| Rust | `rust/src/lib.rs:1` | `rust/src/lib.rs:4` | `rust/src/lib.rs:8` |
| TypeScript | `typescript/src/target.ts:1` | `typescript/src/callers.ts:4` | `typescript/src/callers.ts:8` |
| Python | `python/target.py:1` | `python/callers.py:5` | `python/callers.py:9` |
| Java | `java/src/sample/Target.java:6` | `java/src/sample/Callers.java:7` | `java/src/sample/Callers.java:11` |

Python provides symbols but its stored reference graph can report unavailable edges. The other fixtures expect two incoming references after their language server becomes ready. No fixture has external dependencies.
