# 11. 구현 참고 자료

확인일: 2026-10-01. 공식 문서/프로젝트/논문만 근거로 사용했다. 각 자료는 개념과 API를 참고하는 것이며 코드·문구를 복제하거나 같은 성능/보장을 주장하지 않는다. latest/current URL 내용은 변할 수 있으므로 M0에서 실제 pin 버전과 맞춰 재확인한다.

| ID | 자료 | 읽을 부분 / 우리 설계에 적용 | 그대로 가져오지 않는 것 |
|---|---|---|---|
| R1 | [Axum](https://docs.rs/axum/latest/axum/) | routing, extractors, State, Tower middleware | 자체 정책 엔진을 제공한다는 해석 |
| R2 | [Tokio](https://docs.rs/tokio/latest/tokio/) | async I/O, task, timeout, CPU-bound 작업 주의 | async이면 CPU 작업도 자동으로 빨라진다는 해석 |
| R3 | [SQLx](https://docs.rs/sqlx/latest/sqlx/) | Pool, Transaction, query, migrations | query macro가 권한 의미까지 검증한다는 주장 |
| R4 | [PostgreSQL isolation](https://www.postgresql.org/docs/18/transaction-iso.html) | READ COMMITTED와 REPEATABLE READ의 snapshot 차이 | 외부 앱 DB까지 원자성을 제공한다는 주장 |
| R5 | [PostgreSQL recursive CTE](https://www.postgresql.org/docs/18/queries-with.html) | hierarchy, path/cycle handling | 무제한 graph 탐색 |
| R6 | [proptest](https://docs.rs/proptest/latest/proptest/) | 생성된 입력, shrinking, 회귀 재현 | 테스트가 형식 증명을 대체한다는 주장 |
| R7 | [Zanzibar 논문 소개](https://research.google/pubs/zanzibar-googles-consistent-global-authorization-system/) | 관계 기반 권한과 변경 순서의 중요성 | Google의 규모, latency, external consistency |
| R8 | [SpiceDB consistency](https://authzed.com/docs/spicedb/concepts/consistency) | stale read와 consistency 선택의 tradeoff | ZedToken/API/캐시 아키텍처 호환 |
| R9 | [OWASP BOLA](https://api-security.owasp.org/editions/2023/en/0xa1-broken-object-level-authorization/) | 객체별 검증과 호출 누락 위험 | engine 도입만으로 앱 보안 완료 |
| R10 | [Docker 서비스 분리](https://docs.docker.com/engine/containers/multi-service_container/) | 서비스별 컨테이너, 독립 lifecycle | 모든 서비스를 한 컨테이너로 묶기 |
| R11 | [Compose 네트워킹](https://docs.docker.com/compose/how-tos/networking/) | 서비스 이름, 내부 port, network | private network가 인증을 대체한다는 주장 |
| R12 | [Cedar 검증 설계](https://github.com/cedar-policy/cedar-docs/blob/main/docs/collections/_other/security.md) | reference model와 differential/property testing | Cedar 문법·formal proof를 구현했다고 주장 |
| R13 | [OpenFGA parent-child](https://openfga.dev/docs/modeling/parent-child) | 부모 관계를 통한 권한 상속 이해 | OpenFGA 완전 호환 |
| R14 | [PostgreSQL constraints](https://www.postgresql.org/docs/18/ddl-constraints.html) | composite FK, unique, CHECK 한계 | cycle를 단일 CHECK로 검증 |
| R15 | [Rust 설치/toolchain](https://rust-lang.org/tools/install/) | rustup을 통한 toolchain 관리 | 미확인 버전 pin |

## 나중에 참고할 자료

- [Tonic](https://github.com/grpc/grpc-rust): gRPC 요구가 생긴 뒤 검토.
- [PostgreSQL RLS](https://www.postgresql.org/docs/18/ddl-rowsecurity.html): 추가 tenant 방어 계층 후보. v0.1에는 적용하지 않음.
- [SpiceDB 코드](https://github.com/authzed/spicedb), [OpenFGA 코드](https://github.com/openfga/openfga): API/관계 개념 비교. 현재 제품 전체 구현을 복제할 계획 아님.

외부 자료의 기능은 사실 근거이고, limit·API·DB 선택·마일스톤·성능 목표는 이 프로젝트의 설계 결정이다. 두 종류를 혼동하지 않는다.
