# M1 functional verification

Core source checkpoint: `af1c3b644e05c79c096a708e68cc4977f397f857`.

Command: `PROPTEST_CASES=10000 PROPTEST_RNG_SEED=424242 cargo test -p authz-core --locked m1_`.
M1-01–M1-05 functional checks passed: twelve design scenarios, an independent graph-closure reference, 10,000 generated cases, proof validation/priority, edit-implies-view, additive grants, depth/cycle and work-budget boundaries. No network, database or async runtime is used by the core.

M1-06: Criterion ran successfully with nine deterministic fixtures: direct/team/deny, depth 0/8/32. Single-thread release execution, 10 samples, 100 ms warmup and 200 ms measurement per profile. No SQL or HTTP is included. Command: `cargo bench --locked -p authz-core --bench evaluation`. Host was macOS aarch64; exact CPU and memory metadata were unavailable because the hardware query was denied. Therefore this is an exploratory run, not a fully documented benchmark baseline or an SLA claim. The formal benchmark evidence remains incomplete.
