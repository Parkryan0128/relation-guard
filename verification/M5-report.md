# M5-native — measurement in progress

The production source under measurement is `afc0fe192681eba11e6867f6bf85413baa4598fb`. Native environment and its limitations are defined in the [approved verification plan](../docs/14-native-verification.md). Full M5 completion is not yet claimed. M5-01 functional/native recovery regression passed in a [clean Linux checkout](https://github.com/Parkryan0128/relation-guard/actions/runs/37040865393); original container gates remain deferred.

The initial baseline failed its second single-check measurement: 38 unexpected responses, 16 dropped iterations, and p99 88.26 ms. Wrong decisions: zero. [Raw results](load-before-setup-optimization/manifest.json) are preserved. The pool/SQL metrics justified reducing transaction setup round trips; ADR-012 records that change. The exact same single/batch latency and error thresholds are now being measured again.

[Dependency and license review](security-review.json) found no unresolved advisory in the active server/admin dependency closures for macOS arm64 and Linux arm64/x86_64. Raw cargo-audit does report RUSTSEC-2023-0071 for locked optional rsa 0.9.10; this dependency is not compiled into either binary with the checked features. Feature or target changes require review again. No tracked secret-pattern hit was found; this is not a proof that every possible secret is absent.

The original Docker/Linux resource-limited release benchmark remains deferred. Native results will be reported independently, including every failed numerical gate.
