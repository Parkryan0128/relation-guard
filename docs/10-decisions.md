# 10. Architecture decision records

날짜: 2026-10-01. 상태: 제안된 baseline. 변경 시 근거와 영향을 기록한다.

## ADR-001: 독립 Rust HTTP 서비스

여러 언어의 앱이 사용하고 개발자가 Rust를 경험한다는 목표에 맞춘다. compile-time memory/thread safety는 이점이지만 DB-heavy workload의 속도 우위를 가정하지 않는다. C++/Go/Java도 가능한 선택이다. HTTP 호출 비용과 별도 운영 대상이 생긴다는 대가를 인정한다.

## ADR-002: 고정된 ReBAC 모델

user/team/folder/document + member/parent/viewer/editor로 제한한다. 사용자 정의 DSL, deny, nested team을 제외해 검증 가능한 의미론을 확보한다. 단순 RBAC if문보다 상속·회수·batch·증명·동시성에 집중한다.

## ADR-003: PostgreSQL primary, 캐시 없음

transaction, constraints, set-based query와 recursive hierarchy에 적합하다. 기능이 많다는 이유만으로 선택한 것은 아니며 MySQL도 가능하다. 단일 지원 DB로 테스트 범위를 줄인다. SQLite를 선택하면 여러 authz replica/네트워크 저장소 운영 계약이 달라지므로 MVP에서는 선택하지 않는다.

## ADR-004: 순수 core + snapshot fact loader

SQL은 대상 ancestor와 사용자에 관련된 facts를 준비하고 Rust core가 권한 의미를 평가한다. DB 없이 core를 검증할 수 있다. 모델과 query의 불일치를 막기 위해 real PG end-to-end test와 reference evaluator를 함께 쓴다.

## ADR-005: tenant write lock + expected revision + idempotency

global lock 대신 tenant row lock. 동시 hierarchy 변경을 직렬화하고 stale retry로 grant가 부활하는 문제를 막는다. 충돌/tenant write throughput 저하를 받아들인다. memory mutex는 replica 간 보장 수단으로 사용하지 않는다.

## ADR-006: 서비스 키와 앱의 end-user 인증 분리

신뢰된 백엔드에 tenant-bound scope를 부여한다. 최종 사용자 인증/관리 UI는 범위 밖. 인증을 없애던 과거 chat demo 요구는 이 권한 인프라의 관리 API에 적용하지 않는다.

## ADR-007: HTTP 먼저, batch 포함

언어 독립 연동과 디버깅에 적합하다. gRPC를 동시에 지원하면 계약·테스트·tooling 범위가 늘어난다. batch와 connection reuse를 먼저 구현하며 프로토콜 변경은 측정 후 결정한다.

## ADR-008: 제한된 설명

allow path 하나만 제공하고 scope로 보호한다. 모든 경로 탐색이나 정책 디버거는 범위 밖. 오류를 deny로 위장하지 않으며 caller가 결과를 구별할 수 있게 한다.

## ADR-009: 기본 read consistency, 제한된 보장

primary REPEATABLE READ snapshot. replica/cache/token mode를 제공하지 않는다. 외부 문서 DB와 check→action 경쟁을 해결했다거나 Zanzibar 외부 일관성을 구현했다고 표현하지 않는다.

## ADR-010: 설계 먼저, 독립 저장소

기존 세 프로젝트와 과거 지원 채팅 코드에 연결 의존성을 만들지 않는다. 초기 설계 commit에는 문서·설계 DDL·계약 fixture·문서 검증만 포함했다. GitHub는 우선 비공개, 라이선스는 보류. 벤치마크 결과와 작동하는 API는 구현 후에만 추가한다.

## ADR-011: 검증 가능한 stage gate와 경계 계약

각 stage에 acceptance ID, 환경, 실행 진입점 계획, 통과 조건, evidence 형식을 연결했다. 기존 모호성을 해소한 결정: delete missing의 참조 검사 면제, replay body/header request ID 구분, final-state hierarchy 검증, COMMIT 이후 unknown outcome 우선, direct proof/deny proof 표현, ASCII key_id에서 dot 제외, 운영 listener 분리, 필수 성능 미달 시 M5 미완료. docs/03·04·05·06·07·08·09·12와 schema가 함께 기준을 이룬다. 새로운 기능을 추가하기보다 기존 계약을 구체화한 변경이다.

## ADR-012 — Transaction setup round trips (2026-10-02)

로컬 부하의 최초 baseline에서 단건 2회차의 요청 실패 38건, 미시작 iteration 16건, 성공 응답 p99 약 88ms를 관측했다. 해당 결과는 verification/load-before-setup-optimization에 보존한다. 오류 요청은 facts/evaluator에 도달하지 않았고 pool 획득·트랜잭션 준비 비용이 관측됐다.

트랜잭션 시작, isolation, 두 timeout 설정을 각각 왕복하던 방식 대신 **상수 SQL로 이루어진 BEGIN + SET LOCAL 묶음**을 한 protocol 요청에 보낸다. SQLx 0.8.6의 transaction manager가 사용자 지정 BEGIN 문자열의 ReadyForQuery를 확인하고 rollback guard를 유지하는 구현을 확인했다. 사용자 입력은 이 문자열에 넣지 않는다. tenant/revision/tuple은 계속 bind parameter다.

pool checkout의 선행 ping도 생략한다. 직후 BEGIN이 실제 연결 생존을 검증하며, stale socket은 명시적 DB 오류가 되고 allow/deny로 바뀌지 않는다. DB 연결 차단/복구, lock/statement timeout, snapshot, rollback, COMMIT ack 유실 테스트를 재실행한다. 데이터 SELECT는 여전히 최대 3개이며 일관성 모델, pool 10, acquire 250ms, 요청 2초 한도는 변경하지 않는다.

목표 수치나 실패율 분모는 낮추지 않는다. 같은 workload를 다시 측정하고 실패 회차도 보존한다. 로컬 결과를 Linux 제한 환경 보장으로 표현하지 않는다.


## ADR-013 — 서비스 연결 최소 수와 실패 단계 관측 (2026-10-02)

기존 batch 실패 여섯 건은 첫 data SELECT 전에 발생했고, 같은 5초 구간에서 pool 획득 시간 합계가 약 1.66초 증가했다. 하지만 응답 error code·pool 상태가 저장되지 않아 당시의 외부 지연 원인을 확정할 수 없다. 해당 기록은 실패로 유지한다.

같은 바이너리에 대해 PostgreSQL 응답 전달을 750ms 지연시키는 격리 실험을 했다. 초기 연결 2개에서는 초당 10개 요청 중 4개가 약 250ms 뒤 OVERLOADED로 실패했고 acquire timeout 카운터가 4 증가했다. 연결 10개를 미리 확보하면 동일한 12개 요청이 모두 성공했다. 이는 가능한 실패 경로의 통제된 재현이며, 원래 사건에서 네트워크 지연이 발생했다는 증명은 아니다.

서비스용 풀은 최소 10·최대 10을 유지하도록 변경한다. 관리자 CLI의 최소 연결 수는 0을 유지한다. API deadline·SQL timeout·획득 timeout·데이터 모델·최대 동시성·성능 합격 기준은 변경하지 않는다. 추가 DB 연결 비용을 명시하고, pool timeout/기타 획득 오류/BEGIN 오류 카운터 및 오류 시점의 pool 상태를 기록한다.

보강 후 통제된 지연 테스트와 기능 회귀·동일 batch 부하를 검증한다. 후속 정상 재측정으로 과거 실패 회차를 덮어쓰거나 전체 release gate를 소급 통과시키지 않는다.

## ADR-014 — 짧은 연결 포화의 대기 예산 (2026-10-02)

V1 warmup의 네 503은 모두 pool size=10/idle=0에서 발생했고 acquire timeout 카운터가 네 번 증가했다. 외부 지연의 원인은 당시 기록으로 확정할 수 없다. 동일 바이너리로 DB 응답 전달을 400ms 정지하고 정상 목표 부하인 100 requests/s를 주면 100건 중 10건이 OVERLOADED였다. 900ms 정지에서는 60건이 거절되고 이후 정상 회복했다. 이는 연결 포화와 짧은 대기 예산의 재현이지 과거 지연 원인의 증명이 아니다.

연결 획득의 단일 대기 예산을 250ms에서 500ms로 바꾼다. 연결은 SQLx의 기존 대기 순서를 유지하며 기다리고, timeout 뒤 재시도하거나 SQL/transaction을 재실행하지 않는다. 짧은 DB 지연 뒤 연결이 반환될 때 처리할 여유를 주는 가용성 개선이다. DB 처리 속도 개선이나 503 제거 보장은 아니다. 지속 포화 시 거절까지 최대 250ms 더 걸리는 대가가 있다.

전체 HTTP deadline 2초, statement 1초, lock 500ms, readiness 250ms, 연결 수 10, in-flight 128은 유지한다. HTTP deadline이 먼저 오면 그 시간이 우선한다. 연결 대기 중에는 transaction/권한 snapshot이 없으며 획득 후 최신 snapshot에서 actor와 권한을 검사한다. 별도 무제한 큐, 캐시 또는 fail-open은 도입하지 않는다. p95/p99/오류율 합격 기준도 유지한다.

필수 검증: 같은 400ms 지연에서 100건의 allow/deny 정확성과 오류 0, 900ms 지연에서 유한 시간 내 OVERLOADED 및 회복, 대기 중 read/write 성공과 지속 포화 시 write 무변경, 기존 deadline/동시성/COMMIT 회귀 검사, 같은 S/M 부하 재측정. 이전 실패 증거는 보존한다.
