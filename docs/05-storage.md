# 05. 저장 구조와 구현 알고리즘

> 현재 확장 계약: [사용자 기준 권한 관리](15-managed-authorization.md). 아래 v0.1 설명 중 신뢰된 writer의 무조건 변경, view/edit만 지원, actor 없는 감사/receipt 규칙은 이 확장에서 대체된다.
## 테이블

[현재 DDL](../spec/schema.sql)은 baseline과 schema 2 upgrade를 순서대로 표현한다. 실제 적용은 SQLx migration을 사용하며 기존 데이터 보존 테스트를 통과했다.

- tenants: UUID, 단조 증가 revision. 조직 경계 및 write serialization lock.
- entities: tenant + kind + opaque ID, user의 org_admin 역할. 표시명/문서 본문/개인정보 저장하지 않음.
- relationships: typed resource/relation/subject/subject_relation. 양 끝점은 tenant 포함 composite FK.
- mutation_receipts: tenant + key_id + actor_id + idempotency_key, normalized request hash, 성공 response, 생성 시점.
- audit_events: tenant + revision별 변경 diff, key_id, actor_id, authorization_basis, request ID, timestamp.

API key는 DDL에 넣지 않는다. v0.1은 운영자가 배포하는 secret 설정 파일에서 key_id → tenant/scopes/token digest를 읽는다. 상세 정책은 [운영](07-operations.md).

## DB가 보장하는 것

- 동일 tuple 중복 방지, 허용된 relation/type 조합, tenant 존재와 양 끝점 참조.
- parent resource당 유일성은 부분 unique index. 같은 tenant 안의 parent만 가능.
- resource별 관계 조회의 covering key와 subject 방향 index.
- revision별 감사 행 유일성과 receipt key 유일성.

cycle·최대 depth는 여러 행을 보므로 CHECK로 해결하지 않는다. 서버 write transaction에서 tenant lock 획득 후 final graph에 대해 검증한다. DB 직접 쓰기는 금지한다. RLS는 v0.1에서 도입하지 않는다. tenant-bound SQL·FK·테스트가 격리 수단이며 RLS를 이미 쓴다고 주장하지 않는다.

## Write 알고리즘

1. 서비스 key/scope/actor header/path/body/한도를 검증. canonical hash 계산.
2. READ COMMITTED transaction 시작. `SELECT revision FROM tenants WHERE id=$1 FOR UPDATE`.
3. 같은 tenant/key_id/actor_id/idempotency_key의 receipt를 조회. 동일 hash면 replay, 다르면 충돌.
4. actor의 조직 소속과 모든 operation의 사전 상태 관리 권한을 확인한다. expected_revision을 비교하고 불일치면 rollback.
5. relation deletes → entity deletes → entity puts → relation puts → 조직 role 변경. 모든 query에 tenant 인자.
6. FK/type/parent constraints, 최종 계층, 마지막 조직 관리자 보존을 검증. 폴더 이동은 새 ancestor 길이 + 이동 subtree 최대 깊이로 32 edges를 검사한다. 변경 subtree 순회에도 작업 한도 20,000과 statement timeout을 적용; 초과 시 422 EVALUATION_LIMIT 후 rollback. v0.1은 거대 subtree 이동을 강제로 지원하지 않는다.
7. 실제 row 변경 여부로 changed 결정. 변경 있으면 revision+1 및 실제 diff audit 1행. 요청 순서가 아닌 정렬된 effective diff를 저장.
8. 성공 response와 receipt 저장. no-op도 receipt는 저장하고 audit/revision은 유지.
9. commit 성공을 확인한 후 성공 응답. 연결 단절로 commit 확정 불가하면 unknown outcome.

FK는 처음에는 immediate, deletion은 관계 제거 후 객체 제거하므로 문제없다. 관계가 남은 entity delete는 FK 에러를 ENTITY_IN_USE로 매핑한다. SQL exception 뒤 실패 transaction에서 계속 작업하지 않는다.

## Check fact loading

한 요청당 `REPEATABLE READ READ ONLY` transaction, primary 연결 1개다.

1. tenant revision을 읽어 snapshot을 확정한다.
2. subject user 존재, 대상 resource 존재, 대상들의 ancestor chain을 bounded recursive CTE로 조회한다. 입력 ordinal을 유지한다. SQL에서도 visited path/cycle/depth guard를 둔다.
3. target+ancestor 집합의 viewer/editor/manager 중 direct user 일치 또는 `(team,member,user)`가 존재하는 grant만 set-based 조회한다. 필요한 membership 증거도 같은 statement에서 반환한다.
4. facts를 순수 core에 넘겨 typed rule을 평가하고 proof를 구성한다. transaction을 종료하고 response를 반환한다.

내부 check의 revision/fact loader는 BEGIN/COMMIT/SET 제외 세 SELECT다. HTTP에서는 같은 snapshot에서 actor 소속/역할 SELECT가 하나 추가되어 네 SELECT다. step 2의 존재/ancestor 결과는 하나의 query 또는 동등한 구조로 작성한다. 모든 clause/CTE/join에 tenant 조건을 포함한다. max facts + 1개를 읽어 한도 초과를 감지하며 잘린 결과로 deny를 계산하지 않는다.

read의 parent chain 조회에서 cycle/다중 부모/depth>32 guard에 걸리면 500; 정상 API 입력의 evaluation 예산 초과는 422. 이 구별은 서버가 쓴 유효 계층이라는 invariant와 함께 테스트한다.

## Rust 엔진의 실제 역할

DB는 관계 저장, snapshot, ancestor/fact 집합 준비를 담당한다. Rust core는 action implication, subject matching, allow/deny, deterministic proof, 작업 예산을 담당한다. SQL 하나로 bool만 계산하고 Rust는 포장만 하는 엔진이라고 소개하지 않는다. 동시에 Rust를 강조하려고 DB에서 이미 효율적으로 처리할 일을 N+1 네트워크 탐색으로 바꾸지 않는다.

## 성장 시점

primary 1개, read-only snapshot, 테넌트당 직렬 write는 의도된 시작점이다. hot tenant write contention·큰 subtree·DB pool 포화는 측정할 한계다. closure table, caching, immutable in-memory graph, replicas는 측정 후 consistency 계약을 다시 설계해야 하는 후속 선택지다.

## 구현 세부 확정

- 최종 graph 검증을 위한 parent 변경은 전체 batch의 변경을 적용한 상태에서 수행한다. 한 batch의 두 이동을 각각 이전 상태로 검증하지 않는다. 외부 reader에는 commit 전 중간 상태가 보이지 않는다.
- duplicate 관계의 put은 전체 PK conflict에만 no-op 처리한다. 별도의 one_parent unique index 위반을 `ON CONFLICT DO NOTHING`으로 숨기지 않는다. 서로 다른 parent를 주는 요청은 409다.
- 단일 요청에서 entity put과 이를 참조하는 relation put은 가능하다. 반면 delete_entity와 해당 entity를 참조하는 relation put은 최종 FK 검증에서 422 UNKNOWN_ENTITY다. 삭제 시 남아 있는 기존 관계는 409 ENTITY_IN_USE다.
- revision이 동일한 두 no-op write는 모두 성공할 수 있다. "동시 writer 중 하나만 성공" 테스트는 실제 변경을 수행하는 두 요청에만 적용한다.
- receipt row 정리는 `recorded_at < DB now - 24 hours` 조건으로 최대 1,000행씩 한다. 누적 삭제는 운영 CLI가 반복한다. 정리로 graph revision이나 audit를 수정하지 않는다. v0.1은 상주 정리 scheduler를 제공하지 않는다.
- HTTP disconnect 전에 COMMIT을 보내지 않았다면 transaction을 rollback한다. 이미 보냈다면 bounded finalization으로 상태를 수습하고 결과/불명확 여부를 기록한다. 클라이언트 연결이 사라졌다는 이유로 commit이 취소됐다고 가정하지 않는다.

## Schema 2 확장

`0002_managed_authorization.sql`은 entities.org_admin, manager 관계, actor별 receipt PK와 actor/authorization_basis 감사 필드를 추가한다. 기존 데이터는 member 및 legacy_operator로 보존된다. role과 관계 변경은 모두 동일 tenant lock에 참여한다. 기존 loader의 세 SELECT 외에 HTTP 사용자 검증에서 actor 역할 SELECT가 하나 추가된다. 권한 변경 검사는 사전 상태의 bounded 리소스 집합을 한 번에 로드한다.
