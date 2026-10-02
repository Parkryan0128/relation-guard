# 12. Stage별 acceptance test matrix

상태: **테스트 명세만 존재. 아래 ID는 실행 결과가 아니다.** 모든 ID는 구현 시 최소 하나의 자동 검증 항목으로 연결한다. 절차/환경/evidence는 [roadmap](09-roadmap.md), 규칙은 docs/03·04·06이 기준이다. 버그 수정은 해당 ID의 회귀 case로 남긴다.

## M0 — 계약과 저장 제약

| ID | 입력/절차 | 반드시 확인할 결과 |
|---|---|---|
| M0-01 | pinned workspace build, fmt, Clippy, dependency tree | build 성공, 경고 0, core에 HTTP/DB/runtime 의존 없음, unsafe 금지 |
| M0-02 | 정상 DTO와 unknown/duplicate field, null, ID 0/128/129자, revision 숫자/leading zero/overflow, batch 0/100/101 | 정상만 parse; 실패는 명시된 400/413; OpenAPI와 typed parser 결과 일치 |
| M0-03 | canonical request의 key/op 순서·공백·JSON escape 변형과 expected revision 변경 | 동등 요청 bytes/hash 동일; 의미 변경 hash 다름; 고정 golden bytes/hash |
| M0-04 | 빈 PG에 migration 적용→같은 migration runner 재실행 | 예상 schema와 index 존재; 두번째 적용 무변경; SQL 파일을 맹목적으로 두 번 실행하는 테스트가 아님 |
| M0-05 | invalid typed tuple, cross-tenant FK, duplicate PK, second parent, malformed key_id 직접 SQL | 각각 CHECK/FK/unique 위반. 간접 cycle는 DB만으로 막히지 않음을 명시하고 M2에서 검사 |

## M1 — 순수 평가기

| ID | 입력/절차 | 반드시 확인할 결과 |
|---|---|---|
| M1-01 | authorization-cases.json의 12개 case를 매번 새 snapshot에 적용 | 지정 allow/deny, NOT_FOUND reason 일치; case 사이 mutation 누출 없음 |
| M1-02 | 생성한 작은 forest/team/grant를 독립 reference와 비교 | 모든 decision 일치; minimized counterexample과 seed 저장 |
| M1-03 | edit/view, 무관 tenant, tuple 순서 변경, grant 추가/경로 일부 회수 | docs/08의 모든 속성 통과. parent 변경을 단조성 조건에 섞지 않음 |
| M1-04 | 여러 allow 경로, direct/team 동시 존재, viewer/editor 동시 존재, explain on/off | 정해진 순서의 proof 하나, 경로 실재, deny proof=[], off면 필드 없음 |
| M1-05 | depth 0/32와 손상된 33/cycle, 20,000/20,001 work, duplicate batch items | 경계 성공/오류 정확; 유효한 allow가 먼저 있어도 뒤 오류를 숨기지 않음 |
| M1-06 | core Criterion direct/team/inherited/deny baseline | CPU/버전/seed와 결과 기록; DB/HTTP 시간을 core 성능으로 표기하지 않음 |

## M2 — 실제 PostgreSQL과 동시성

| ID | 입력/절차 | 반드시 확인할 결과 |
|---|---|---|
| M2-01 | 유효 entity+relation batch 및 마지막 단계 invalid relation/entity deletion | 정상 한 번 commit; 실패 graph/revision/audit/receipt 모두 무변경; delete missing은 no-op |
| M2-02 | 같은 key replay, 다른 hash, no-op, expired receipt, revision 최대값 | replay 결과 불변, hash conflict, no-op receipt만 생성; stale grant 부활 없음; overflow rollback |
| M2-03 | 같은 revision의 실제 변경 2개를 barrier로 동시 실행 | 성공 1, 409 1, revision +1. no-op 2개는 별도 case로 모두 성공 가능 |
| M2-04 | 동일 key write 2개; grant 응답 유실→revoke→grant retry | 중복 graph/audit 없음; original receipt replay이며 현재 상태는 revoked |
| M2-05 | A→B/B→A 경쟁, 두 parent, self-cycle, 여러 이동, subtree depth 33 | final graph 기준 위반 거절; stale expected revision이면 conflict 우선; 유효 이동의 중간 상태 노출 없음 |
| M2-06 | snapshot 획득→revoke commit→facts read, revoke 후 새 snapshot, batch 중간 write | 첫 check는 old coherent snapshot, 새 check는 deny, batch는 revision 하나 |
| M2-07 | 1/100 targets, 공유 ancestor, 같은 ID 다른 tenant, 10,001 matching grants | 데이터 SELECT ≤3, tenant 누출 0, 한도 초과 422; limit으로 자른 상태에서 deny 금지 |
| M2-08 | receipt 24시간 이내/초과 정리, authz key 변경, 객체 삭제/재등록 | 이내 보존·초과만 삭제·한 번 ≤1,000개; graph revision 무변경; 과거 grant 복원 없음 |

## M3 — 실제 HTTP와 실패 계약

| ID | 입력/절차 | 반드시 확인할 결과 |
|---|---|---|
| M3-01 | write/check/batch/GET 정상·오류와 unsupported method/media/route | status/code/body/header가 계약 일치; batch 입력 순서·중복 유지; schema 검증 |
| M3-02 | 없음/오류 token, dot 포함 key_id, 다른 tenant, scope 조합 전체 | 401/403 및 startup 오류 정확; explain/read/audit 권한 분리; raw token/proof 누출 없음 |
| M3-03 | invalid JSON, duplicate key, unknown parameter, body 262,144/262,145 bytes, depth 16/17 | 지정 한도/우선순위로 400/413, 압축 body는 415; 무제한 allocate 없음 |
| M3-04 | 같은 key replay와 새 request ID, GET cursor 변조/필터 mismatch/마지막 page | replay body 원본·header 현재 ID; replay flag; bad cursor 400, 종료 cursor=null |
| M3-05 | pool 점유/tenant lock 점유/slow SQL/request deadline/DB 종료 | OVERLOADED/DEADLINE_EXCEEDED/DATABASE_UNAVAILABLE 구분; 허위 allow/deny 없음; 다음 정상 요청 성공 |
| M3-06 | COMMIT 직전 disconnect, COMMIT 전송 뒤 ack 유실을 test proxy/hook으로 제어 | 전자는 rollback; 후자는 성공 receipt 재시도 또는 unknown. commit 실패를 추측하지 않음 |
| M3-07 | 128개 요청을 barrier로 점유 후 129번째 요청; batch 후반 강제 evaluator 오류 | queue 없이 503; health 접근 유지; batch 부분 결과 없음; 해제 후 처리 재개 |

## M4 — 배포와 사용자 연동

| ID | 입력/절차 | 반드시 확인할 결과 |
|---|---|---|
| M4-01 | 고유 Compose project에 clean image build/bootstrap | root 아닌 runtime; DB/metrics host port 미공개; readiness 정상; secret 이미지 미포함 |
| M4-02 | Python client에서 entity/grant/check/revoke/check, unknown outcome retry/409 | 언어 독립 HTTP 동작, deny/error에 보호 작업 미실행, 409 blind retry 금지 |
| M4-03 | 정상 writes 뒤 authz와 DB container 재시작 | volume 데이터·revision·receipt 유지; graph가 초기 seed로 덮이지 않음 |
| M4-04 | 격리된 환경의 backup→빈 PG restore→새 key provision→검증 | tuple/audit/revision/결정 일치; stale credential 401; 검증 전 traffic 차단 |
| M4-05 | 새 key 추가 rollout→옛 key 제거 rollout, SIGTERM drain | 모든 instance에서 옛 key 401; 신규 연결 종료; in-flight 최대 10초; commit outcome 기록 |
| M4-06 | schema version 불일치, DB 장애, 운영 listener probing | healthz 200, readyz 503; 내부 error/credential 비노출; 복구 시 readyz 200 |

## M5 — 릴리스 증거

| ID | 입력/절차 | 반드시 확인할 결과 |
|---|---|---|
| M5-01 | clean checkout에서 M0–M4 전부 재실행 | 필수 case 실패/skip 0; report가 실제 테스트 commit을 가리킴 |
| M5-02 | 고정 S dataset의 단건과 100-item batch, 각 3회 | docs/09의 각 회차 latency/error gate 통과; 3회 평균으로 실패 회차를 숨기지 않음 |
| M5-03 | M dataset, mixed hot/distributed tenant, stress ramp, 오류 후 recovery | 실제 한계와 conflict/saturation 기록; offered load·완료·실패·dropped 모두 기록; 복구 후 정확성 유지 |
| M5-04 | fault/retry/security regression 및 dependency/license/secret review | allow 누출·권한 부활 없음, 미해결 release-blocking 문제 없음; 근거 있는 finding 처리 |
| M5-05 | README/지원 API/알려진 한계/restore/성능 문서와 구현 대조 | 미구현 기능 광고 없음, 재현 명령·환경·artifact 존재, scope 변경의 ADR 추적 가능 |

## 테스트가 해결하지 않는 문제

각 case의 실행 여부는 verification 보고서에서 구분한다. property tests는 형식 증명이 아니며, local Docker 부하는 운영 SLA가 아니다. 타 앱의 DB commit과 권한 check 사이 경쟁은 v0.1에서 제거하지 않는다. 의도한 제한을 버그처럼 숨기지 않고 integration test/report에서 명시한다.
