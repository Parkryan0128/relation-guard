# 사용자 기준 권한 관리 — `org-doc-v2`

이 문서는 승인된 V1 확장의 구현 계약이다. 기존 `org-doc-v1` 측정·fixture는 과거 검증 기록이며 새로운 모델의 통과 증거가 아니다.

## 신뢰 경계와 API

로그인은 기존 앱이 담당한다. 모든 data API 요청은 tenant-bound 서비스 키와 정확히 하나의 `X-Actor-Id`를 요구한다. 앱은 로그인 세션에서 얻은 ID만 이 헤더에 넣으며, 외부 사용자가 보낸 동일 이름의 헤더를 그대로 전달하지 않는다. 서비스 키 소유자는 신원을 증언하는 신뢰된 백엔드다. 이 헤더는 자체 인증 토큰이 아니며, 탈취된 서비스 키나 악의적인 신원 제공자를 방어한다고 주장하지 않는다. 서비스는 ID 형식, 조직 소속과 실제 권한을 DB에서 검증한다. 일반 사용자가 authz에 직접 접속하는 방식은 지원하지 않는다.

서비스 scope는 기존대로 필요한 최소 호출 범위이며 사용자 자격을 대체하지 않는다. 누락/중복/잘못된 actor 헤더는 401, 등록되지 않은 actor나 권한 부족은 403. HTTP의 actor 없는 write 경로는 제거한다. 관리자 CLI만 별도 privileged DB credential로 최초 관리자 지정/긴급 관리자 복구를 수행한다.

## 고정 역할과 규칙

- 등록된 user는 조직 member. `set_org_role(user_id, role=admin|member)`는 조직 관리자만 수행한다.
- 조직 admin은 모든 존재하는 리소스의 view/edit/manage 및 조직의 user/team 관리를 할 수 있다. 없는 리소스는 admin에게도 NOT_FOUND deny다.
- folder/document의 manager는 edit와 view를 포함한다. viewer/editor는 manage를 포함하지 않는다. manager도 직접 사용자/팀과 폴더 상속을 지원한다.
- 마지막 조직 admin을 강등하거나 삭제할 수 없다. self-demotion은 다른 admin이 남는 경우 허용한다. 삭제된 user를 재등록해도 과거 admin 지위는 복원하지 않는다.
- 팀 생성/삭제와 팀 멤버 추가/제거는 조직 admin만 한다. 문서 manager는 팀을 조작해 접근을 확대할 수 없다.
- 기존 리소스의 grant/revoke/delete/move는 변경 전 상태에서 actor의 manage를 요구한다. 여러 operation에서 새로 얻은 권한으로 같은 요청의 다른 operation을 정당화할 수 없다.
- 새 root는 조직 admin만 만든다. 일반 manager는 변경 전부터 존재하며 manage 가능한 폴더 아래에 새 리소스를 만들 수 있다. 새 부모까지 한 번에 만드는 복합 생성은 조직 admin만 허용한다. 생성한 리소스는 부모 권한을 상속하며 같은 요청에서 명시적 grant를 부여할 수 있다.
- 이동은 기존 리소스와 새 부모 모두 변경 전 manage를 요구한다. 일반 manager의 root로 분리는 금지한다. 리소스 삭제에 필요한 parent 제거는 허용한다. 이동하면 새 부모 권한을 상속한다.
- 관계 삭제는 그 경로만 회수한다. 다른 팀/직접/상속 경로가 남아 있으면 접근은 유지한다. explicit deny와 상속 차단은 없다.
- member는 자기 자신에 대한 check/batch-check만 할 수 있다. 조직 admin은 다른 사용자도 조회할 수 있다. explain에는 기존 explain scope도 필요하다.
- 관계 목록은 리소스 manage를 요구한다. user/team 관계 조회와 조직 전체 audit 조회는 조직 admin만 허용한다. revision은 등록된 member면 조회할 수 있다.

## 원자성, 재시도와 감사

write는 tenant row lock → actor에 결속된 receipt 확인 → actor 소속/자격 확인 → expected revision 확인 → 모든 operation의 사전 상태 권한 검사 → 변경/최종 invariant 검사 → revision/audit/receipt → commit 순이다. 같은 tenant의 모든 관리 변경과 운영자 복구도 동일 lock을 사용한다. 박탈이 먼저 commit되면 뒤의 새 변경은 거부된다. 먼저 승인된 변경은 먼저 commit되고 그 뒤에 박탈된다.

receipt의 키는 `(tenant, service key, actor, idempotency key)`다. 동일 actor의 완료된 작업 재시도는 이미 저장된 결과만 반환하며 권한을 다시 부여하지 않는다. 자기 탈퇴/자기 강등 후에도 자기 receipt 재조회는 가능하다. 다른 actor는 그 receipt를 재생할 수 없다. 운영자/구버전 receipt는 별도 빈 actor namespace다.

실제 변경의 감사 행에 actor_id, 호출 key_id, 권한 근거(org_admin 또는 관리한 리소스), diff를 기록한다. 운영자 초기 설정/복구는 actor=null과 operator 근거로 명시한다. 기존 audit는 legacy_operator로 표시하고 소급해서 사용자 신원을 꾸며 넣지 않는다.

## 전환과 제한

DB migration 2로 manager 관계, 조직 역할, actor별 receipt, audit 근거를 추가한다. 기존 user는 member로 유지하며 임의 사용자를 admin으로 승격하지 않는다. 기존 조직은 운영자가 `bootstrap-admin UUID USER_ID`로 최초 admin을 지정한 뒤 새 API를 사용한다. 이미 admin이 있으면 bootstrap은 실패한다. `recover-admin UUID USER_ID`는 운영자만 사용하는 명시적 복구 명령이며 감사 기록을 남긴다.

기존 최대 100 ops/checks, 그래프/SQL/요청 시간 한도는 유지한다. 조직 역할 지정도 100 ops 한도에 포함한다. 배치 관리 검사에 필요한 서로 다른 리소스가 100개를 넘으면 EVALUATION_LIMIT로 전체 거부한다. 요청자가 볼 수 없는 자료는 오류 상세로 노출하지 않는다.

## 구현·검증 단계

1. Core/DDL: manager 포함/불포함, 직접/팀/상속, admin과 없는 객체, role payload와 중복 검증, migration 전후 보존 검사.
2. Store: admin/manager/member 허용·거부 표, 혼합 batch 전부 rollback, 자신 승격/팀 조작/폴더 이동 우회, 마지막 admin, actor별 receipt와 재생, audit 근거.
3. 동시성: 박탈→변경/변경→박탈 순서, 동시 마지막 admin 강등, 같은 revision 경쟁, 동일 actor receipt 경쟁, 다른 actor key 충돌 없는 격리. sleep이 아니라 DB lock/barrier로 순서를 통제한다.
4. HTTP: actor 누락/중복/형식/미등록/다른 tenant, scope와 actor 둘 다 검사, 무관리 권한 변경과 조회 차단, 기존 우회 API 없음, 오류/타임아웃/commit 불명확과 재시도.
5. 통합: CLI 초기화/복구, 기존 데이터 migration, Python client, 문서/OpenAPI, 전체 회귀·복구·Docker 배포·성능 검증. 실행하지 않은 검증은 완료로 표시하지 않는다.
