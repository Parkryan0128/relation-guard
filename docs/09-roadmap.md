# 09. 구현 순서와 완료 체크리스트

현재 완료된 작업은 설계 문서 작성뿐이다. 아래 항목은 전부 미구현이다. 다음 구현 요청 시 M0부터 시작한다. 예상 일정 대신 검증 가능한 산출물로 단계를 나눈다.

## M0 — 계약 확정과 골격

- [ ] Rust stable/edition, crate 호환성, toolchain/lockfile pin 확인.
- [ ] core/store/server/admin workspace 생성, fmt/Clippy CI.
- [ ] docs/04를 OpenAPI 3.1 schema로 옮기고 request/response fixture 검증.
- [ ] core의 ID/action/relation 타입과 strict parser, limits 상수.
- [ ] PostgreSQL 설계 DDL을 실제 migration으로 전환하고 빈 DB 적용 테스트.

완료: 정상/잘못된 DTO fixture가 계약대로 검증되고 DB constraints가 실제 PG에서 검증됨. 아직 권한 엔진 완성이라고 부르지 않음.

## M1 — 순수 권한 평가기

- [ ] fixed model rules, direct/team/inherited grants, proof, budgets.
- [ ] reference evaluator, fixture runner, proptest.
- [ ] core microbench scaffold. 아직 SQL/HTTP와 결합하지 않음.

완료: 모든 권한 fixture와 property test 통과, core에 네트워크/DB/runtime 의존성 없음.

## M2 — 저장·원자적 관계 변경

- [ ] tenant admin CLI와 revision bootstrap.
- [ ] transactional write, composite tenant constraints, graph validation.
- [ ] idempotency canonicalization/receipts, audit, expected revision.
- [ ] snapshot fact loader, bounded batch SQL, 실제 PG integration tests.
- [ ] receipt 만료 정리 명령; 감사 기록은 삭제하지 않음.

완료: mutation rollback·stale retry·cycle concurrency·snapshot consistency 테스트 통과. 이 단계의 동시성 버그를 HTTP로 가리지 않음.

## M3 — 네트워크 서비스와 보안

- [ ] Axum routes, service-key scopes, DTO/error mapping.
- [ ] check/batch/write/revision/relationships/audit.
- [ ] deadlines, body/work/in-flight limits, graceful shutdown.
- [ ] health/readiness/metrics와 structured logs.
- [ ] HTTP contract/auth/cross-tenant/DB failure integration tests.

완료: 다른 프로세스에서 호출 가능, 오류가 allow로 처리되지 않음, scope 없는 explain 불가.

## M4 — 통합과 배포

- [ ] multi-stage Docker, non-root image, Compose private network/volume.
- [ ] 비밀값 없는 env/secret template와 key 생성 절차.
- [ ] Python 표준 라이브러리 예제: create→grant→check→revoke→check.
- [ ] 앱에서 check error/deny를 처리하는 연동 예제, revision conflict 처리 안내.
- [ ] backup/restore 실험, migration 운영 및 key rotation 문서.

완료: 새 환경에서 문서만 보고 시나리오 재현. Rust 설치 없는 client도 HTTP 호출 가능.

## M5 — 증거와 릴리스

- [ ] docs/08의 성능 profile, p95/p99/오류/자원 측정 보고서.
- [ ] correctness gates 재확인, secret/dependency/license 점검.
- [ ] README에 실제 지원 기능·한계·실행법·측정 결과 반영.
- [ ] 모든 목표의 달성/미달 사유 공개. revision과 모델 호환 정책 명시.

완료: 재현 가능한 v0.1 candidate. public 전환/오픈소스 라이선스는 사용자가 결정한다.

## v0.2 이후 후보 — 자동 포함 아님

gRPC, language SDK, 캐시+revision consistency, 접근 가능 리소스 검색, in-memory graph 최적화, dynamic model, RLS, 다중 replica 부하 실험. 먼저 실제 병목/사용 요구가 있어야 한다. DSL·분산 캐시·서비스 mesh를 경력용 장식으로 추가하지 않는다.
