# 03. 권한 의미론 — 고정 모델 `org-doc-v1`

이 문서는 권한 결과의 source of truth다. 모델을 임의로 OpenFGA/Cedar와 동일하다고 해석하지 않는다.

## 객체와 테넌트

객체는 `(tenant_id, kind, id)`로 식별한다. kind는 user, team, folder, document 중 하나다. 조직은 tenant 자체이며 조직 소속만으로 모든 문서를 볼 수 없다. user registry에 객체가 있다는 사실도 권한 부여가 아니다.

ID는 종류 내에서 유일한 ASCII opaque string, 1–128자, `[A-Za-z0-9][A-Za-z0-9._-]{0,127}`. 대소문자를 구분하고 Unicode 정규화/URL path 의미를 추가하지 않는다. 외부 앱 ID를 그대로 또는 안정적으로 매핑해서 전달한다.

## 허용되는 관계

| resource kind | relation | subject kind | subject_relation |
|---|---|---|---|
| team | member | user | 빈 문자열 |
| folder/document | parent | folder | 빈 문자열 |
| folder/document | viewer/editor | user | 빈 문자열 |
| folder/document | viewer/editor | team | member |

`document:d1#parent@folder:f1`은 d1이 f1 아래에 있음을 뜻한다. `folder:f1#editor@team:t1#member`는 t1의 멤버 집합에 편집 권한을 부여한다. team 자체를 로그인 사용자처럼 check할 수 없다.

그 외 조합은 INVALID_RELATION으로 거절한다. 관계 양쪽 객체는 같은 tenant에 존재해야 한다. wildcard, nested team, owner bypass, explicit deny는 없다.

## 판정 규칙

`matches(user, subject)`는 직접 사용자 일치 또는 해당 팀의 member 관계 존재다.

```text
edit(u, r) = 직접 editor 관계에서 matches(u)
             OR (r의 parent가 p이고 edit(u, p))

view(u, r) = edit(u, r)
             OR 직접 viewer 관계에서 matches(u)
             OR (r의 parent가 p이고 view(u, p))
```

등록된 user와 folder/document에만 적용한다. 조직 내에 있어도 허용 경로가 없으면 deny. 다른 관계가 그대로인 상태에서 유효한 grant 추가는 기존 allow를 deny로 만들지 않는다.

parent는 리소스당 최대 하나다. root folder와 parent 없는 document도 유효하며 직접 권한만 적용된다. folder self-parent와 간접 cycle은 거절한다. 최대 parent edge 수는 대상에서 root까지 32개다.

## 회수와 삭제

관계 삭제는 그 경로만 제거한다. 팀에서 제거해도 사용자에게 직접 공유된 editor는 남는다. viewer 삭제 후 editor가 남으면 view는 계속 허용된다. 접근 완전 차단이나 deny 우선순위는 없다.

parent 이동은 같은 write에서 old parent 삭제 + new parent 등록으로 표현한다. 최종 상태를 기준으로 단일 부모·cycle·depth를 검증한다. 여러 write로 나누면 중간 상태가 노출되므로 권장하지 않는다.

객체 삭제는 연결된 모든 관계를 같은 transaction에서 먼저 명시적으로 삭제한 경우만 허용한다. 참조가 남으면 409 ENTITY_IN_USE. subtree cascade delete는 제공하지 않는다. 조회의 unknown subject/resource는 결과 `deny`와 `NOT_FOUND` reason으로 반환하고, 설명용 scope에서도 다른 tenant를 검색하지 않는다.

## 설명과 오류

`explain=true`는 allow를 증명하는 경로 하나를 반환한다. 모든 경로의 열거 또는 최단 경로 탐색 서비스는 아니다. 후보 우선순위는 가까운 resource, 직접 user, team ID 사전순, relation 사전순으로 고정하고 마지막 tie는 전체 tuple 정렬로 푼다. view에서 viewer/editor 모두 증명이 될 수 있다. deny는 NO_MATCHING_RELATION 또는 NOT_FOUND만 반환한다.

DB 오류, 깊이/작업 한도 초과, corrupt graph는 deny 판단이 아니다. HTTP error를 반환하고 client는 보호 작업을 실행하지 않는다. error에 allowed 필드를 넣지 않는다. 상세 데이터 손상은 로그/운영자에게만 노출한다.

판단에는 `model_version`과 `evaluated_revision`이 들어간다. revision은 snapshot을 설명하는 값이며 권한 증명 token이나 영구 access token이 아니다.

## 판정·증명 세부 계약

- ancestor depth는 parent edge 개수다. root의 depth=0, root 바로 아래 문서는 1. 폴더 이동 검증은 하위 document까지 포함해 최종 상태의 최대 depth를 검사한다.
- candidate proof 정렬 키는 `(대상부터 grant까지의 parent edge 수, direct user=0/team=1, subject_id의 ASCII 순서, relation의 ASCII 순서, 전체 tuple)`이다. SQL collation이나 입력 순서에 결과를 맡기지 않는다.
- 반환 proof는 대상→grant 리소스 순의 parent tuples, grant tuple, 필요한 경우 member tuple 순서다. 직접 user grant의 proof는 1개 tuple이다.
- `explain=false`에서는 proof 필드를 생략한다. `explain=true`이고 deny면 `proof: []`; allow면 위 경로 하나다. batch에도 item별로 동일 규칙을 적용한다.
- 입력 전체의 형식/권한과 로드 결과의 무결성/한도를 먼저 검증한다. 앞 item이 allow라고 해서 뒤 item의 오류를 숨기지 않는다. 어느 item이든 평가가 실패하면 batch 전체가 오류다.
- read에서 cycle/다중 부모/depth>32를 발견하면 저장 invariant 손상이므로 500 INTERNAL_ERROR. write가 그런 상태를 만들려 하면 409 INVALID_HIERARCHY. 정상 데이터에서 평가 작업 예산만 초과하면 422 EVALUATION_LIMIT.
- 삭제 후 같은 ID를 새 write에서 재등록하는 것은 허용하지만 과거 grant를 복원하지 않는다. 오래된 integration event를 새 의도로 재발행하는 문제는 caller가 식별해야 한다.
