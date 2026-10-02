# 09. 구현 순서와 완료 체크리스트

구현을 시작했다. M0–M3 및 native recovery 검증 결과는 [현재 증거](../verification/status.md)를 따른다. 아래 stage 완료는 모든 필수 case와 증거가 있을 때만 인정한다.

## 모든 stage에 적용하는 gate

`scripts/verify.sh`는 구현된 Rust/실제 PostgreSQL/TCP 회귀 검사를 실행한다. `scripts/test-stage.sh`는 별도 stage gate이며 미완료 case가 있는 stage의 PASS를 거부한다. 정확한 재현 방법은 docs/13-running.md를 따른다.

각 gate는 [테스트 매트릭스](12-acceptance-tests.md)의 해당 ID 전부를 요구한다. stage가 올라가면 이전 stage gate도 누적 실행한다. 실패/skip/환경 없음은 PASS가 아니다. 실패한 stage는 미완료로 남기고 관련 결과를 보고한다. 예상 오류를 검증하는 negative test는 지정된 오류와 무변경 조건까지 확인해야 성공이다.

증거 파일은 구현 때 `verification/MN-report.md`에 저장한다. 테스트한 commit SHA, toolchain/DB/image 버전, 명령, 실행 시각, case별 PASS/FAIL/SKIP, 로그·JUnit·benchmark artifact 위치를 기록한다. 인증정보와 원본 payload는 제거한다. CI artifact 보존 기간은 30일, 요약 보고서는 Git에 남긴다. 이 문서의 체크박스는 증거 없이 체크하지 않는다.

## M0 — 계약 확정과 골격

- [x] Rust stable/edition, crate 호환성, toolchain/lockfile pin 확인.
- [x] core/store/server/admin workspace 생성, fmt/Clippy CI.
- [x] docs/04를 OpenAPI 3.1 schema로 옮기고 request/response fixture 검증.
- [x] core의 ID/action/relation 타입과 strict parser, limits 상수.
- [x] PostgreSQL 설계 DDL을 실제 migration으로 전환하고 빈 DB 적용 테스트.

완료: 정상/잘못된 DTO fixture가 계약대로 검증되고 DB constraints가 실제 PG에서 검증됨. 아직 권한 엔진 완성이라고 부르지 않음.

### M0 테스트 방법과 통과 기준

- 환경: pinned Rust toolchain + 빈 PostgreSQL 18 DB. 각 테스트에는 고유 DB/schema를 할당한다.
- 진입점: `scripts/test-stage.sh M0`. 내부에서 fmt check, Clippy warnings denied, workspace build, typed DTO/OpenAPI fixture tests, migration/constraint tests 실행.
- 필수 case: M0-01–M0-05. DB CHECK/FK/unique는 실패 SQLSTATE와 rollback 후 상태까지 확인한다.
- 통과: contract의 valid/invalid fixture 전부 일치, migration 재실행이 schema를 바꾸지 않음, first-party unsafe 금지 확인. core 성능이나 권한 결과는 이 단계에서 달성했다고 보고하지 않는다.
- 증거: verification/M0-report.md, pin 파일/lockfile, schema 검증 및 DB test 로그.

## M1 — 순수 권한 평가기

- [x] fixed model rules, direct/team/inherited grants, proof, budgets.
- [x] reference evaluator, fixture runner, proptest.
- [x] core microbench scaffold. 아직 SQL/HTTP와 결합하지 않음.

완료: 모든 권한 fixture와 property test 통과, core에 네트워크/DB/runtime 의존성 없음.

### M1 테스트 방법과 통과 기준

- 환경: DB/HTTP 없는 core 테스트. 정상 fixture마다 독립 초기 상태를 사용한다.
- 진입점: `scripts/test-stage.sh M1`. core unit/proptest와 reference comparison, proof validation 실행. property당 PR에서 1,000 cases, stage gate에서 10,000 cases; seed와 minimized counterexample 저장.
- 필수 case: M1-01–M1-06. 경계는 바로 아래/동일/바로 위를 모두 확인한다.
- 통과: 차등 결과 불일치 0, 모든 allow proof 검증 성공, malformed snapshot은 error, core runtime/DB 의존 0. Criterion은 baseline 기록만 하며 이 단계의 통과 시간을 임의로 정하지 않는다.
- 증거: verification/M1-report.md, property seed/회귀 fixture, baseline artifact.

## M2 — 저장·원자적 관계 변경

- [x] tenant admin CLI와 revision bootstrap.
- [x] transactional write, composite tenant constraints, graph validation.
- [x] idempotency canonicalization/receipts, audit, expected revision.
- [x] snapshot fact loader, bounded batch SQL, 실제 PG integration tests.
- [x] receipt 만료 정리 명령; 감사 기록은 삭제하지 않음.

완료: mutation rollback·stale retry·cycle concurrency·snapshot consistency 테스트 통과. 이 단계의 동시성 버그를 HTTP로 가리지 않음.

### M2 테스트 방법과 통과 기준

- 환경: pinned PostgreSQL, 별도 pool/connection으로 reader와 writer 실행. transaction boundary에 test-only barrier 삽입; sleep으로 순서를 맞추지 않는다.
- 진입점: `scripts/test-stage.sh M2`. 모든 PG integration과 동시성 schedule 실행. stage gate에서 각 schedule 100회 반복하고 첫 실패에 fail; flaky retry로 녹색 처리하지 않는다.
- 필수 case: M2-01–M2-08. rollback 확인 대상은 graph, revision, audit, receipt 네 가지 모두다.
- 통과: commit 횟수/최종 revision/증명 경로가 예상과 일치, batch 데이터 SELECT ≤3, 잘린 facts로 정상 deny를 반환하지 않음.
- 증거: verification/M2-report.md, query count와 EXPLAIN 출력, sanitized transaction schedule 로그.

## M3 — 네트워크 서비스와 보안

- [x] Axum routes, service-key scopes, DTO/error mapping.
- [x] check/batch/write/revision/relationships/audit.
- [x] deadlines, body/work/in-flight limits, graceful shutdown.
- [x] health/readiness/metrics와 structured logs.
- [x] HTTP contract/auth/cross-tenant/DB failure integration tests.

완료: 다른 프로세스에서 호출 가능, 오류가 allow로 처리되지 않음, scope 없는 explain 불가.

### M3 테스트 방법과 통과 기준

- 환경: 실제 TCP listener의 Axum 서버 + 실제 PostgreSQL. mock storage를 이용한 unit test만으로 대체하지 않는다.
- 진입점: `scripts/test-stage.sh M3`. OpenAPI request/response validation, HTTP auth/contract test, deterministic fault injection 실행.
- 필수 case: M3-01–M3-07. test-only fault hook은 production build에 포함하지 않는다.
- 통과: 모든 status/code/envelope 일치, unknown outcome을 rollback으로 오인하지 않음, credential/tenant/proof 누출 0, overload와 deadline 뒤 연결 풀 정상 복구.
- 증거: verification/M3-report.md, sanitized HTTP transcripts, scope matrix, fault recovery 로그.

## M4 — 통합과 배포

현재 실행 경로는 [승인된 native 계획](14-native-verification.md)의 M4-native다. 아래 Docker 관련 조건은 원래 배포 gate로 남긴다. native 복구 통과는 컨테이너 배포 통과를 뜻하지 않는다.

- [ ] multi-stage Docker, non-root image, Compose private network/volume.
- [x] 비밀값 없는 env/secret template와 key 생성 절차.
- [x] Python 표준 라이브러리 예제: create→grant→check→revoke→check.
- [x] 앱에서 check error/deny를 처리하는 연동 예제, revision conflict 처리 안내.
- [x] native backup/restore 실험, migration 운영 및 key rotation 문서.

완료: 새 환경에서 문서만 보고 시나리오 재현. Rust 설치 없는 client도 HTTP 호출 가능.

### M4 테스트 방법과 통과 기준

- 환경: 고유 Compose project/임시 volume. 기존 개발·사용자 DB는 건드리지 않는다. Rust가 없는 별도 client 컨테이너에서 HTTP 호출.
- 현재 진입점: `scripts/test-stage.sh M4`는 native 누적 검증을 실행한다. 원래 clean Compose gate의 별도 진입점은 `scripts/test_deployment.py`이며 배포 검증 재개 시 실행한다.
- 필수 case: M4-01–M4-06. 테스트 정리 명령은 자신이 만든 project/volume에만 적용한다.
- 통과: 새 환경에서 문서만으로 재현, DB 재시작 뒤 데이터 유지, restore 뒤 기대 결과 일치, 제거한 키는 모든 실행 replica에서 401, 개발 host에 DB/metrics port 비공개.
- 증거: verification/M4-report.md, image digest, 구성 검토 결과, 복구 전후 revision/tuple count/check 결과. secret은 포함하지 않는다.

## M5 — 증거와 릴리스

**실행 결과: M5-native 전 시나리오 실행 완료, acceptance 미달.** 마지막 batch 회차에서 3,001건 중 503 6건(0.20%)으로 오류율 목표를 넘었다. 아래 산출물은 완성했지만 stage 통과를 의미하지 않는다. 상세 수치와 한계는 verification/M5-report.md를 따른다.

`scripts/test-stage.sh M5`는 현재 M5-native 경로다. 아래 Linux 제한 환경의 정식 배포 성능 gate는 별도로 남는다. 수치 기준은 동일하게 유지하며 미달 회차를 평균으로 숨기지 않는다.

- [x] docs/08의 성능 profile, p95/p99/오류/자원 측정 보고서.
- [x] correctness gates 재확인, secret/dependency/license 점검.
- [x] README에 실제 지원 기능·한계·실행법·측정 결과 반영.
- [x] 모든 목표의 달성/미달 사유 공개. revision과 모델 호환 정책 명시.

완료: 재현 가능한 v0.1 candidate. public 전환/오픈소스 라이선스는 사용자가 결정한다.

### M5 테스트 방법과 통과 기준

- 환경: docs/08의 고정 Linux 자원/데이터/부하 조건. 다른 환경 결과를 정식 baseline으로 대체하지 않는다.
- 진입점: `scripts/test-stage.sh M5`. 이전 gate 전체 + load/fault/recovery + release review 실행.
- 필수 case: M5-01–M5-05. 단건 profile과 batch profile은 각 3회 독립 측정한다.
- 통과: 정확성/보안 gate 전부 통과, 단건 3회 각각 p95≤20ms·p99≤50ms·예상 밖 실패율<0.1%, batch 3회 각각 p95≤100ms·예상 밖 실패율<0.1%. 불가능하거나 미달이면 M5 미완료. 측정 후 숫자를 몰래 낮추지 않으며 목표 변경은 이유/새 조건을 ADR로 먼저 기록하고 다시 측정한다.
- 증거: verification/M5-report.md, sanitized metrics와 benchmark manifest, 의존성/secret 검토, 알려진 한계. 취약점 검토 미해결은 release gate를 통과하지 못한다.

## v0.2 이후 후보 — 자동 포함 아님

gRPC, language SDK, 캐시+revision consistency, 접근 가능 리소스 검색, in-memory graph 최적화, dynamic model, RLS, 다중 replica 부하 실험. 먼저 실제 병목/사용 요구가 있어야 한다. DSL·분산 캐시·서비스 mesh를 경력용 장식으로 추가하지 않는다.
