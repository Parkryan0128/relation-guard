# M1 verification — functional PASS; native baseline recorded

Production source: `afc0fe192681eba11e6867f6bf85413baa4598fb`. [Regression log](regression.log), [Criterion output](core-benchmark.log).

`PROPTEST_CASES=10000 PROPTEST_RNG_SEED=424242 cargo test -p authz-core --locked m1_` passed all four test functions: twelve design scenarios, independent reference comparison over 10,000 generated cases, deterministic proof priority, edit-implies-view and additive grants, and depth/cycle/work limits. The core has no async runtime, database, network, or wall-clock dependency.

M1-01–M1-05: PASS. M1-06: native exploratory baseline recorded using `cargo bench --locked -p authz-core --bench evaluation`. Nine fixtures cover direct/team/deny at depths 0, 8, and 32; 10 samples, 100 ms warmup, 200 ms measurement each. Point estimates range from 83.7 ns for depth-0 deny to 48.6 µs for depth-32 team access. These are pure evaluation times, excluding HTTP and SQL.

Host: macOS Darwin 25.2, arm64, eight logical CPUs; exact CPU model and physical memory were unavailable under the host permission policy. No CPU or memory cap was imposed. Under the [native verification plan](../docs/14-native-verification.md), this is an explicitly limited local baseline, not Linux resource-limited release evidence or an HTTP latency claim.
