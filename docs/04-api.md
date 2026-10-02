# 04. HTTP API 계약

> 현재 확장 계약: [사용자 기준 권한 관리](15-managed-authorization.md). 아래 v0.1 설명 중 신뢰된 writer의 무조건 변경, view/edit만 지원, actor 없는 감사/receipt 규칙은 이 확장에서 대체된다.
상태: org-doc-v2 구현 계약. canonical JSON 예시는 [contracts.json](../spec/examples/contracts.json). M0에서 이 문서를 OpenAPI 3.1로 옮기고 fixture를 schema로 검증한다.

## 공통

Prefix `/v1/tenants/{tenant_id}`. tenant_id는 UUID. JSON은 UTF-8, Content-Type application/json. API key는 `Authorization: Bearer <token>`에만 넣는다. secret/query string 금지. 모든 revision은 JSON decimal string으로 전달해 타 언어의 정수 정밀도 차이를 피한다. 0–9223372036854775807, leading zero 금지(0 제외).

인증된 key는 정확히 한 tenant와 scope 집합에 결속된다. 다른 tenant path는 403. 인증 실패 401, scope 부족 403. UUID/JSON/unknown field/잘못된 enum은 400. request ID는 매 HTTP 시도마다 서버가 UUID로 생성해 X-Request-Id header에 반환한다. body request_id는 기본적으로 같지만 write receipt replay에서는 원래 ID를 유지한다. 클라이언트가 보낸 request ID는 식별자로 신뢰하지 않는다.

| Method / path suffix | 필요 scope | 역할 |
|---|---|---|
| POST `/write` | relations:write | 객체·관계의 원자적 변경 |
| POST `/check` | check | 단일 판정 |
| POST `/batch-check` | check | 같은 subject의 여러 판정 |
| GET `/revision` | relations:read | 현재 tenant revision, model version |
| GET `/relationships` | relations:read | 대상 resource를 지정한 관계 목록 |
| GET `/audit` | audit:read | revision 순 변경 내역 |

explain은 추가로 explain scope 필요. check scope만으로 원본 관계 목록이나 proof를 얻을 수 없다. tenant/key provisioning은 공개 API 없이 신뢰된 admin CLI로 수행한다.

## Write

필수 header `Idempotency-Key`: 1–128 ASCII `[A-Za-z0-9._-]+`.

필수 body: `expected_revision`, `operations` 배열. operation은 put_entity, delete_entity, put_relation, delete_relation, set_org_role 중 하나이며 명시적인 tagged union이다. entity는 `{kind,id}`. relation은 `{resource,relation,subject,subject_relation}`이며 direct subject는 `subject_relation:""`을 사용한다.

- 1–100 ops. 같은 entity key나 동일 relation tuple이 두 번 나타나면 400 DUPLICATE_OPERATION. old/new parent는 다른 tuple이므로 동시 delete/put 가능.
- 순서에 의존하지 않는 최종 상태 의미론. relation delete → entity delete → entity put → relation put 순으로 적용하고 최종 검증한다. put된 관계는 살아 있는 객체를 참조해야 한다.
- put existing / delete missing은 no-op. delete_relation은 tuple의 타입/형식을 검증하되 삭제 대상과 양 끝점이 이미 없어도 no-op이다. put_relation만 최종 상태에서 양 끝점 존재를 요구한다. 존재하지 않는 entity의 delete도 no-op. 삭제와 재생성을 한 요청에 중복 기입할 수 없다.
- expected revision이 현재와 다르면 409 REVISION_CONFLICT + current_revision. 처음 쓰기도 expected_revision=0.
- 동일 key+동일 정규화 payload 재시도는 원래 receipt를 반환. key 재사용에 다른 payload는 409 IDEMPOTENCY_CONFLICT. receipt에는 이전 request_id가 유지되고 header의 X-Request-Id는 현재 시도 ID다.
- 성공 200: changed boolean, revision, model_version, request_id. 실제 graph가 바뀌면 tenant revision을 정확히 1 증가. no-op은 revision 유지.
- 유효하지 않은 relation/references는 422, 객체 사용 중·다중 parent·cycle·depth 위반은 409. 하나라도 실패하면 graph/revision/audit/receipt 전부 rollback.
- commit outcome 불명확 시 503 WRITE_OUTCOME_UNKNOWN. 실패를 곧 미반영으로 해석하지 말고 같은 key/payload로 재시도.

정규화 hash는 parse/검증된 typed request에서 아래 canonicalization v1 규칙으로 얻은 bytes를 SHA-256으로 계산한다. whitespace와 입력 operation 순서만 다른 요청은 같은 payload다. expected_revision도 hash에 포함된다. idempotency key는 인증 key_id, actor_id와 tenant 내에서 유일하다. key 교체 후 자동 retry는 새 작업이므로 revision 충돌 처리부터 다시 한다.

## Check

body: subject(user만), resource(folder/document만), action(view/edit/manage), explain(default false). 성공 200은 `decision:"allow"|"deny"`, reason, evaluated_revision, model_version, request_id, 선택적 proof 배열. allow reason은 관계 경로면 MATCHING_RELATION, 조직 관리자면 ORG_ADMIN이다. proof는 explain 권한과 요청이 모두 있을 때만 제공한다.

존재하지 않는 local 객체는 200 deny NOT_FOUND. 기존 객체에 경로가 없으면 200 deny NO_MATCHING_RELATION. 다른 tenant의 객체를 찾아서 구분하지 않는다. unsupported kind/action은 400이며 정상 거부와 구별한다.

## Batch check

body: 단일 subject, checks 배열(1–100, 각 `{resource,action}`), explain(default false). 결과는 입력 순서/중복을 그대로 유지한다. 전체 요청이 한 snapshot/revision을 사용한다. 일부 객체 없음은 해당 item의 정상 deny. 형식 오류·DB 오류·평가 한도는 전체 요청 오류이며 부분 allow를 내보내지 않는다. client는 누락되거나 깨진 응답을 사용하지 않는다.

## 관리 조회

`/relationships?resource_kind=folder&resource_id=f1&limit=50&after=...`: resource 필수, limit 1–100. 정렬 키 `(relation,subject_kind,subject_id,subject_relation)`. cursor는 이 키와 tenant/resource/filter를 담는 versioned base64url 값으로 검증한다. 권한을 부여하는 credential은 아니며 변조된 cursor도 query scope를 바꿀 수 없다. 요청마다 별도 snapshot이므로 여러 페이지가 하나의 시점이라는 보장은 없다. 없는 resource는 빈 목록.

`/audit?after_revision=0&limit=50`: 실제 변경만 revision 오름차순. 최대 100. key_id, actor_id, authorization_basis, committed revision, timestamp, 변경 diff와 request ID를 반환. no-op은 감사 변경 행이 없다. 원시 secret 저장 금지. `/revision`은 초기 sync와 conflict 복구용이며 compare-and-write를 대체하지 않는다.

## 제한과 오류

초기 hard limits: body 256 KiB(262,144 bytes), JSON nesting depth 16, ops 100, checks 100, ancestor depth 32 edges, load된 unique resource 최대 3,300, matching grant facts 최대 10,000, work units 최대 20,000. core의 work unit 정의와 early-return 규칙은 아래 세부 계약을 따른다. proof는 최대 34 tuple(32 parent + grant + membership). JSON depth는 root object를 1로 세고, object/array 진입마다 1 증가한다. scalar는 depth를 늘리지 않는다. parser도 입력 깊이/배열 크기를 제한한다.

초기 운영 설정: 요청 deadline 2초, DB statement timeout 1초, lock timeout 500ms, pool acquire timeout 250ms, DB pool max 10/replica, in-flight max 128/replica, graceful drain 10초. request가 남은 deadline보다 긴 query를 시작하지 않도록 조정한다. write SQL 및 read SQL timeout은 같은 기준. 값은 성능 근거가 생기면 문서와 테스트를 같이 변경한다.

| HTTP | code | 의미 |
|---|---|---|
| 400 | INVALID_REQUEST / DUPLICATE_OPERATION | 잘못된 입력 |
| 404 / 405 / 415 | NOT_FOUND / METHOD_NOT_ALLOWED / UNSUPPORTED_MEDIA_TYPE | 경로·메서드·Content-Type 오류 |
| 401 / 403 | UNAUTHENTICATED / FORBIDDEN | 서비스 인증/범위 실패 |
| 409 | REVISION_CONFLICT / IDEMPOTENCY_CONFLICT / ENTITY_IN_USE / INVALID_HIERARCHY / LAST_ADMIN | 상태 충돌 |
| 413 | PAYLOAD_TOO_LARGE | body 제한 |
| 422 | INVALID_RELATION / UNKNOWN_ENTITY / EVALUATION_LIMIT | 의미 오류 또는 한도 |
| 503 | OVERLOADED / DATABASE_UNAVAILABLE / WRITE_OUTCOME_UNKNOWN / REVISION_EXHAUSTED | 일시 장애·포화 또는 revision 한계 |
| 504 | DEADLINE_EXCEEDED | 응답 deadline 초과 |
| 500 | INTERNAL_ERROR | 손상·버그 |

error body는 `{error:{code,message,request_id,details?}}`. message는 사람이 읽는 설명으로 문구 안정성을 보장하지 않으며 client는 HTTP status와 code로 분기한다. raw SQL·secret·stack trace는 외부에 반환하지 않는다. rate limit을 넣기 전 429를 광고하지 않는다. client timeout은 서버 deadline보다 길게 설정하되 자신의 전체 요청 예산 안에 둔다.

## 모호성 제거: 요청·재시도·오류의 정확한 규칙

### 입력과 validation 순서

지원 경로의 요청 처리 순서는 (1) transport/body 크기 한도 (2) credential 검증 (3) tenant path와 scope (4) Content-Type·strict JSON·필드/배열 한도 (5) transaction과 도메인 검증이다. 일찍 실패하면 뒤 단계의 정보는 반환하지 않는다. unknown route는 404, 알려진 route의 잘못된 method는 405. POST의 Content-Type은 application/json 또는 application/json; charset=utf-8만 받는다. 중복 JSON object key, null인 필수 필드, unknown field, 압축 request body, 잘못된 UTF-8은 거절한다. 압축/미지원 media type은 415, 나머지 payload 형식 오류는 400이다.

`subject_relation`은 모든 관계 DTO에서 필수다. direct는 빈 문자열이고 누락/null과 다르다. `/relationships`의 limit 기본 50, `/audit`의 limit 기본 50·after_revision 기본 "0". 알 수 없거나 중복된 query parameter는 400. `check`와 `batch-check`만 explain 기본 false를 허용한다.

한 단계에서 여러 오류가 있으면 validation은 entity/tuple canonical 순서로 진행하고 첫 오류를 반환한다. write transaction 내에서는 receipt lookup → expected revision → mutation/final graph 검증 순서를 고정한다. stale revision인 요청의 graph 위반은 revision conflict보다 우선하지 않는다.

### Canonicalization v1

write에는 optional field/default가 없다. 모든 object key를 ASCII 오름차순으로 재귀 정렬하고 string은 JSON escaping, separator는 `,`와 `:`만 사용한다. whitespace와 마지막 newline을 넣지 않는다. 각각의 operation을 같은 규칙으로 직렬화한 UTF-8 byte 순서로 정렬하고 operations 배열에 넣은 뒤 전체 `{expected_revision,operations}`를 직렬화한다. 숫자·부동소수점을 쓰지 않으며 revision은 검증된 문자열이다. 동일 target의 operation 중복은 정렬 전에 거절한다. 객체 field 순서·공백·operation 순서·동등한 JSON escape 차이는 동일 hash를 만들어야 한다. M0에서 golden byte/hash fixture를 추가한다.

### Write 및 receipt

receipt replay는 현재 credential 인증/scope 검사를 통과한 뒤만 가능하다. replay 응답은 원래 성공 body 그대로이며 과거 revision이다. header `Idempotency-Replayed: true`로 표시하고 fresh 성공에는 false를 반환한다. X-Request-Id는 현재 시도, body request_id는 원래 mutation 시도다. replay를 현재 권한 상태 확인으로 사용하지 않는다.

receipt는 성공 응답만 저장한다. 실패 응답은 cache하지 않는다. no-op은 audit/revision 변경 없이 receipt만 저장한다. changed=true인데 현재 revision이 최대 bigint면 503 REVISION_EXHAUSTED로 전부 rollback; no-op과 기존 receipt replay는 가능하다.

COMMIT을 아직 보내지 않았다면 statement/전체 deadline은 504, lock timeout과 pool acquire timeout/in-flight 포화는 503 OVERLOADED다. transaction은 rollback하고 오염된 연결을 pool에 반환하지 않는다. COMMIT을 보낸 뒤 성공/실패가 확정되지 않았다면 deadline보다 WRITE_OUTCOME_UNKNOWN(503)을 우선한다. 이미 HTTP 연결이 끊겨 응답할 수 없다면 완료를 꾸며내지 않고 서버 로그에 outcome을 기록한다. client 입장에서는 어떤 연결 실패/timeout도 커밋 실패의 증거가 아니다.

known DB connection 장애는 503 DATABASE_UNAVAILABLE, 예측하지 못한 SQL 오류는 500 INTERNAL_ERROR다. hierarchy/constraint 오류는 명시된 409/422로 매핑한다. unsupported 관계 형태 삭제는 422; 이미 없는 유효 관계 삭제는 성공 no-op이다.

### 작업 예산

check/batch의 공통 요청 예산은 20,000 units다. 각 item마다 방문하는 target/ancestor state 1회당 1, 검사하는 grant 후보 1개당 1, team grant의 membership 확인 1회당 1을 차감한다. target/root도 state에 포함한다. 모든 관련 후보를 deterministic 순서로 검사한 뒤 첫 유효 proof를 선택하므로 allow early-return으로 한도 검사를 건너뛰지 않는다. duplicate batch item도 다시 charge한다. 네트워크/SQL fact는 deduplicate할 수 있지만 예산은 입력 item 기준이다.

10,000 matching grants는 요청 전체에서 unique tuple 기준이며 membership 증거는 grant 수에 중복 가산하지 않는다. write graph 검증은 check 예산과 별도로 최대 20,000 node visits; ancestor/subtree 재방문도 1회씩 차감한다. write의 ops 제한과 audit diff 제한은 서로 동일한 최대 100이다. 한도 초과는 전체 rollback 또는 check 전체 오류다.

### 관리 조회 응답

`/revision`: `{revision,model_version,request_id}`.
`/relationships`: `{relationships,evaluated_revision,next_cursor,request_id}`. 더 없으면 next_cursor=null; limit+1개를 읽어 다음 페이지 존재를 판별한다.
`/audit`: `{events,evaluated_revision,next_after_revision,request_id}`. event는 `{revision,key_id,actor_id,authorization_basis,request_id,recorded_at,effective_changes}`. 더 없으면 next_after_revision=null, 더 있으면 마지막 반환 revision. 시간은 RFC3339 UTC이며 recorded_at은 transaction의 기록 시각으로 정확한 commit timestamp가 아니다.

각 GET은 한 read-only snapshot에서 revision과 내용을 함께 읽는다. 페이지 간 snapshot 일치는 보장하지 않는다. cursor는 최대 2,048 bytes, base64url unpadded JSON `{v:1,tenant_id,resource_kind,resource_id,last:[relation,subject_kind,subject_id,subject_relation]}`. path/filter 불일치·지원 안 되는 version·타입·크기 오류는 400. cursor는 인증/권한 검사를 대체하지 않는다.

## V1 관리 확장 요청

모든 data API에는 `X-Actor-Id` 헤더가 정확히 하나 필요하다. 인증된 백엔드가 로그인 세션의 사용자 ID를 증언한다. scope 검사를 통과해도 사용자 자격 검사는 생략하지 않는다. `set_org_role` payload는 `{"op":"set_org_role","user_id":"u1","role":"admin"}`이며 role은 admin/member뿐이다. 동일 user의 role 변경 중복 또는 같은 요청의 user 삭제와 role 변경 조합은 DUPLICATE_OPERATION이다.

자기 check는 member에게 허용하고 타인의 check는 조직 admin만 가능하다. 관계 조회는 리소스 manager 또는 조직 admin, audit은 조직 admin만 가능하다. 모든 변경 자격과 receipt 재시도 순서는 [관리 확장 계약](15-managed-authorization.md)을 따른다.
