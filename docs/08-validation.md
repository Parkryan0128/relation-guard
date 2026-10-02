# 08. 검증 계획과 성능 기준

## 정확성 gate

[기대 결과 fixture](../spec/examples/authorization-cases.json)를 test runner로 실행한다. 단순 예시 fixture 외에 아래를 모두 추가한다.

| 영역 | 필수 시나리오 |
|---|---|
| 기본 규칙 | direct viewer/editor, edit→view, 없는 권한 deny, unknown ID deny |
| 팀 | membership 추가/제거, 다른 팀, direct grant가 남는 회수 |
| 계층 | 여러 단계 상속, root, parent 없는 document, 이동 후 이전 권한 소멸 |
| 격리 | tenant A/B 동일 ID, 다른 tenant key/path, join tenant 누락 회귀 |
| 무결성 | dangling edge, invalid typed edge, 두 parent, self/indirect cycle |
| 한도 | depth 32 성공/33 거절, batch 100/101, body/facts/work 초과 |
| write | atomic rollback, no-op, 중복 op, expected_revision 충돌 |
| retry | 동일 key replay, 다른 payload 충돌, receipt 만료 후 stale retry |
| 인증 | 없음/오류 key, scope 부족, explain 정보 누출, rotation |
| 장애 | DB 종료, statement timeout, pool 포화, client disconnect, shutdown |

Rust 단위 테스트에 더해 SQLx+실제 PostgreSQL integration을 사용한다. SQLite/H2나 mock store만으로 isolation/FK/CTE를 검증했다고 하지 않는다. CI는 PostgreSQL service container, 로컬도 같은 major를 사용한다.

## 속성 기반/차등 테스트

작은 유효 forest, users/teams, grants를 proptest로 생성한다. reference evaluator는 단순 반복 ancestor 탐색과 membership 집합 검사로 독립 구현한다. optimized evaluator와 로직을 공유해 양쪽 버그가 같아지는 테스트를 피한다. 동일 typed facts/같은 모델의 결과를 비교한다.

속성: edit allow ⇒ view allow; 무관한 tenant 변경 불변; 같은 edges의 순서 불변; batch 결과와 동일 snapshot 단건 의미 일치; grant 추가의 단조성(계층/객체 변경 없음); 삭제한 경로 외 surviving proof가 있으면 allow 유지; 모든 allow proof가 실제 edges와 규칙으로 검증 가능.

OpenFGA와 비교는 optional 후속 검증이며 동일 semantics를 매핑한 부분만 비교할 수 있다. v0.1 완료에 외부 엔진 가동을 요구하지 않는다. 정책 의미는 자체 reference/fixture가 기준이다 [R6,R12].

## 동시성 테스트 — sleep 대신 barrier

- 두 writer가 같은 revision에서 시작: 정확히 하나만 실제 변경 commit, 나머지 409.
- folder A→B와 B→A 동시 변경: lock으로 직렬화되어 cycle 허용 안 됨.
- check snapshot 획득 후 revoke commit, 이어서 check facts 조회: old snapshot의 일관된 결과 허용.
- revoke 성공을 기다린 후 새 check: 남은 경로가 없으면 deny.
- 같은 key 동시 write: graph/audit 한번만 변경, 같은 receipt.
- grant 응답 유실→revoke→오래된 grant retry: 권한 부활 없음.
- batch 중간 write: 모든 item은 동일 revision.

transaction 시작이 아닌 첫 snapshot statement 시점을 test hook으로 제어한다. read-after-write 검증에 임의의 delay를 넣지 않는다.

## 성능 실험

측정 전 목표값이며 현재 결과가 없다. 고정 환경 baseline: authz container 2 vCPU/512 MiB, PostgreSQL 2 vCPU/1 GiB, 같은 Linux host의 private bridge network. 전용 load generator의 자원을 별도 확보하고 CPU 모델, storage, host memory, OS, Rust/DB 버전, image digest와 commit을 기록한다. 개발 Mac 결과는 별도 표기한다.

데이터 S: 1 tenant, 1,000 users, 100 teams, 1,000 folders, 10,000 documents, 약 50,000 relationships, 최대 깊이 8. M: 총 약 1,000,000 relationships의 10 tenants, 정확한 분포/seed/allow ratio는 generator manifest로 기록. edge에는 parent/member/grant를 모두 포함한다.

프로파일:

1. in-memory core: direct/team/inherited/deny와 depth 0/8/32별 Criterion 측정.
2. HTTP 단건: pooled keep-alive, 50:50 allow/deny, S dataset, warm DB buffer, 100 requests/s, 60초 warmup 후 5분 측정 3회.
3. batch: 100 checks/request, 10 requests/s. 단건 100회의 순차/동시 호출과 비교하되 requests/s와 decisions/s를 구별.
4. mixed: 95% check/5% write, hot tenant와 tenant 분산 각각. write client는 revision conflict 수와 재시도 비용도 포함.
5. stress: load를 단계적으로 증가시켜 latency/error/pool saturation이 발생하는 지점을 기록. 극한 throughput을 정상 운영 SLO처럼 쓰지 않음.

초기 성능 가설: profile 2에서 p95 ≤20ms, p99 ≤50ms, 예상 밖 error <0.1%; profile 3에서 batch p95 ≤100ms. 네트워크 포함 값이다. 이 값은 업계 보장이나 Rust 우위의 증거가 아니다. M0 이후 첫 baseline에서 달성 가능성을 검토하고 실패 시 원인/변경 근거를 남긴다. 기능상 완료와 성능 목표 달성 여부는 별도로 보고한다. M0–M4가 통과해도 M5의 필수 성능 목표가 미달이면 release gate는 미완료다. 목표 변경은 ADR에 근거/조건을 먼저 기록하고 재측정하며 몰래 낮추지 않는다.

필수 기록: end-to-end p50/p95/p99, request/decision throughput, 4xx/5xx/timeout/409 개수, DB statement 수·시간, pool wait, core 시간, CPU/RSS, DB CPU/IO, cold/warm 상태. dropped iteration도 실패로 기록하며 성공 요청만의 percentile로 전체 성공을 주장하지 않는다.

최적화 gate: batch fact loading은 입력 수에 비례해 DB round trip을 늘리지 않음(데이터 SELECT 목표 ≤3). 인덱스는 EXPLAIN (ANALYZE, BUFFERS) 증거로 평가. 성능 때문에 tenant/consistency 검사를 빼지 않는다.

## 측정·CI 규칙 확정

필수 stage별 매핑은 [acceptance matrix](12-acceptance-tests.md). 모든 PR은 구현된 stage까지 fmt/lint/unit/property/integration/contract를 실행한다. PR property는 속성당 1,000 cases, stage gate는 10,000 cases. 동시성 schedule 100회 반복은 M2 및 release gate에 포함한다. full performance gate는 M5와 성능에 영향을 주는 변경 때 실행한다.

benchmark generator 기본 seed는 424242다. S와 M의 정확한 생성 manifest·유효 user/resource 집합·expected decision을 버전 관리한다. 실행 전에 실제 cardinality와 expected allow/deny를 검증한다. parameter가 달라지면 기존 report와 동일 dataset이라고 부르지 않는다.

HTTP baseline은 고정 arrival rate로 측정하며 VU 수 증가로 목표 부하를 숨기지 않는다. unexpected failure ratio 분모는 scheduled requests, 분자는 transport 오류 + 예상하지 않은 HTTP/body/decision + 시작하지 못한 dropped iterations다. expected deny는 실패가 아니다. correctness mismatch는 비율과 관계없이 즉시 gate 실패다. successful request latency percentile과 failure/dropped 수를 함께 보고하며 응답 없는 요청을 0ms로 넣지 않는다.

statement count는 core fact loading의 revision/entities+ancestors/grants queries만 세며 BEGIN/COMMIT/SET과 readiness query를 제외한다. query instrumentation은 재시도 포함 실제 수행 수를 기록한다. >3이면 최적화 gate 실패이고 사유만 적고 통과 처리하지 않는다.

성능 report에서 M dataset/mixed/stress는 관측 항목이고 정량 pass 기준은 S single/batch에만 적용한다. 모든 profile에서 잘못된 allow/deny, panic, 데이터 손실은 실패다. 과부하의 의도된 503은 stress 결과에 포함하되 정상 baseline 실패를 제외하는 핑계로 쓰지 않는다.
