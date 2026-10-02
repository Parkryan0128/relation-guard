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

기존 세 프로젝트와 과거 지원 채팅 코드에 연결 의존성을 만들지 않는다. 현재 commit에는 문서·설계 DDL·계약 fixture·문서 검증만 포함한다. GitHub는 우선 비공개, 라이선스는 보류. 벤치마크 결과와 작동하는 API는 구현 후에만 추가한다.

## ADR-011: 검증 가능한 stage gate와 경계 계약

각 stage에 acceptance ID, 환경, 실행 진입점 계획, 통과 조건, evidence 형식을 연결했다. 기존 모호성을 해소한 결정: delete missing의 참조 검사 면제, replay body/header request ID 구분, final-state hierarchy 검증, COMMIT 이후 unknown outcome 우선, direct proof/deny proof 표현, ASCII key_id에서 dot 제외, 운영 listener 분리, 필수 성능 미달 시 M5 미완료. docs/03·04·05·06·07·08·09·12와 schema가 함께 기준을 이룬다. 새로운 기능을 추가하기보다 기존 계약을 구체화한 변경이다.

## ADR-012 — Transaction setup round trips (2026-10-02)

로컬 부하의 최초 baseline에서 단건 2회차의 요청 실패 38건, 미시작 iteration 16건, 성공 응답 p99 약 88ms를 관측했다. 해당 결과는 verification/load-before-setup-optimization에 보존한다. 오류 요청은 facts/evaluator에 도달하지 않았고 pool 획득·트랜잭션 준비 비용이 관측됐다.

트랜잭션 시작, isolation, 두 timeout 설정을 각각 왕복하던 방식 대신 **상수 SQL로 이루어진 BEGIN + SET LOCAL 묶음**을 한 protocol 요청에 보낸다. SQLx 0.8.6의 transaction manager가 사용자 지정 BEGIN 문자열의 ReadyForQuery를 확인하고 rollback guard를 유지하는 구현을 확인했다. 사용자 입력은 이 문자열에 넣지 않는다. tenant/revision/tuple은 계속 bind parameter다.

pool checkout의 선행 ping도 생략한다. 직후 BEGIN이 실제 연결 생존을 검증하며, stale socket은 명시적 DB 오류가 되고 allow/deny로 바뀌지 않는다. DB 연결 차단/복구, lock/statement timeout, snapshot, rollback, COMMIT ack 유실 테스트를 재실행한다. 데이터 SELECT는 여전히 최대 3개이며 일관성 모델, pool 10, acquire 250ms, 요청 2초 한도는 변경하지 않는다.

목표 수치나 실패율 분모는 낮추지 않는다. 같은 workload를 다시 측정하고 실패 회차도 보존한다. 로컬 결과를 Linux 제한 환경 보장으로 표현하지 않는다.
