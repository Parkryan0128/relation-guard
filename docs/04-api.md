# 04. HTTP API 계약

상태: v0.1 구현 전 규격. canonical JSON 예시는 [contracts.json](../spec/examples/contracts.json). M0에서 이 문서를 OpenAPI 3.1로 옮기고 fixture를 schema로 검증한다.

## 공통

Prefix `/v1/tenants/{tenant_id}`. tenant_id는 UUID. JSON은 UTF-8, Content-Type application/json. API key는 `Authorization: Bearer <token>`에만 넣는다. secret/query string 금지. 모든 revision은 JSON decimal string으로 전달해 타 언어의 정수 정밀도 차이를 피한다. 0–9223372036854775807, leading zero 금지(0 제외).

인증된 key는 정확히 한 tenant와 scope 집합에 결속된다. 다른 tenant path는 403. 인증 실패 401, scope 부족 403. UUID/JSON/unknown field/잘못된 enum은 400. request ID는 서버가 생성해 body 및 X-Request-Id header에 반환한다.

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

필수 body: `expected_revision`, `operations` 배열. operation은 put_entity, delete_entity, put_relation, delete_relation 중 하나이며 명시적인 tagged union이다. entity는 `{kind,id}`. relation은 `{resource,relation,subject,subject_relation}`이며 direct subject는 `subject_relation:""`을 사용한다.

- 1–100 ops. 같은 entity key나 동일 relation tuple이 두 번 나타나면 400 DUPLICATE_OPERATION. old/new parent는 다른 tuple이므로 동시 delete/put 가능.
- 순서에 의존하지 않는 최종 상태 의미론. relation delete → entity delete → entity put → relation put 순으로 적용하고 최종 검증한다. put된 관계는 살아 있는 객체를 참조해야 한다.
- put existing / delete missing은 no-op. 존재하지 않는 entity의 delete도 no-op. 삭제와 재생성을 한 요청에 중복 기입할 수 없다.
- expected revision이 현재와 다르면 409 REVISION_CONFLICT + current_revision. 처음 쓰기도 expected_revision=0.
- 동일 key+동일 정규화 payload 재시도는 원래 receipt를 반환. key 재사용에 다른 payload는 409 IDEMPOTENCY_CONFLICT. receipt에는 이전 request_id가 유지되고 header의 X-Request-Id는 현재 시도 ID다.
- 성공 200: changed boolean, revision, model_version, request_id. 실제 graph가 바뀌면 tenant revision을 정확히 1 증가. no-op은 revision 유지.
- 유효하지 않은 relation/references는 422, 객체 사용 중·다중 parent·cycle·depth 위반은 409. 하나라도 실패하면 graph/revision/audit/receipt 전부 rollback.
- commit outcome 불명확 시 503 WRITE_OUTCOME_UNKNOWN. 실패를 곧 미반영으로 해석하지 말고 같은 key/payload로 재시도.

정규화 hash는 parse/검증된 typed request에서 object key 정렬, operation key 정렬 후 stable JSON을 SHA-256으로 계산한다. whitespace와 입력 operation 순서만 다른 요청은 같은 payload다. expected_revision도 hash에 포함된다. idempotency key는 인증 key_id와 tenant 내에서 유일하다. key 교체 후 자동 retry는 새 작업이므로 revision 충돌 처리부터 다시 한다.

## Check

body: subject(user만), resource(folder/document만), action(view/edit), explain(default false). 성공 200은 `decision:"allow"|"deny"`, reason, evaluated_revision, model_version, request_id, 선택적 proof 배열. proof는 explain 권한과 요청이 모두 있을 때만 제공한다.

존재하지 않는 local 객체는 200 deny NOT_FOUND. 기존 객체에 경로가 없으면 200 deny NO_MATCHING_RELATION. 다른 tenant의 객체를 찾아서 구분하지 않는다. unsupported kind/action은 400이며 정상 거부와 구별한다.

## Batch check

body: 단일 subject, checks 배열(1–100, 각 `{resource,action}`), explain(default false). 결과는 입력 순서/중복을 그대로 유지한다. 전체 요청이 한 snapshot/revision을 사용한다. 일부 객체 없음은 해당 item의 정상 deny. 형식 오류·DB 오류·평가 한도는 전체 요청 오류이며 부분 allow를 내보내지 않는다. client는 누락되거나 깨진 응답을 사용하지 않는다.

## 관리 조회

`/relationships?resource_kind=folder&resource_id=f1&limit=50&after=...`: resource 필수, limit 1–100. 정렬 키 `(relation,subject_kind,subject_id,subject_relation)`. cursor는 이 키와 tenant/resource/filter를 담는 versioned base64url 값으로 검증한다. 권한을 부여하는 credential은 아니며 변조된 cursor도 query scope를 바꿀 수 없다. 요청마다 별도 snapshot이므로 여러 페이지가 하나의 시점이라는 보장은 없다. 없는 resource는 빈 목록.

`/audit?after_revision=0&limit=50`: 실제 변경만 revision 오름차순. 최대 100. key_id, committed revision, timestamp, 변경 diff와 request ID를 반환. no-op은 감사 변경 행이 없다. 원시 secret 저장 금지. `/revision`은 초기 sync와 conflict 복구용이며 compare-and-write를 대체하지 않는다.

## 제한과 오류

초기 hard limits: body 256 KiB, ops 100, checks 100, ancestor depth 32 edges, load된 unique resource 최대 3,300, matching grant facts 최대 10,000, work units 최대 20,000. 1 work unit은 core가 검사하는 fact/ancestor state 하나. 재방문에도 count한다. proof는 최대 34 tuple(32 parent + grant + membership). parser도 입력 깊이/배열 크기를 제한한다.

초기 운영 설정: 요청 deadline 2초, DB statement timeout 1초, lock timeout 500ms, pool acquire timeout 250ms, DB pool max 10/replica, in-flight max 128/replica, graceful drain 10초. request가 남은 deadline보다 긴 query를 시작하지 않도록 조정한다. write SQL 및 read SQL timeout은 같은 기준. 값은 성능 근거가 생기면 문서와 테스트를 같이 변경한다.

| HTTP | code | 의미 |
|---|---|---|
| 400 | INVALID_REQUEST / DUPLICATE_OPERATION | 잘못된 입력 |
| 401 / 403 | UNAUTHENTICATED / FORBIDDEN | 서비스 인증/범위 실패 |
| 409 | REVISION_CONFLICT / IDEMPOTENCY_CONFLICT / ENTITY_IN_USE / INVALID_HIERARCHY | 상태 충돌 |
| 413 | PAYLOAD_TOO_LARGE | body 제한 |
| 422 | INVALID_RELATION / UNKNOWN_ENTITY / EVALUATION_LIMIT | 의미 오류 또는 한도 |
| 503 | OVERLOADED / DATABASE_UNAVAILABLE / WRITE_OUTCOME_UNKNOWN | 일시 장애·포화 |
| 504 | DEADLINE_EXCEEDED | 응답 deadline 초과 |
| 500 | INTERNAL_ERROR | 손상·버그 |

error body는 `{error:{code,message,request_id,details?}}`. raw SQL·secret·stack trace는 외부에 반환하지 않는다. rate limit을 넣기 전 429를 광고하지 않는다. client timeout은 서버 deadline보다 길게 설정하되 자신의 전체 요청 예산 안에 둔다.
