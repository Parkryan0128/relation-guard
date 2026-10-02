# 01. 범위와 제품 목표

상태: 제안된 v0.1 구현 계약. 사용자 검토 후 바뀔 수 있지만 구현 중 임의 확대하지 않는다.

## 문제

서비스마다 팀 소속, 폴더 상속, 직접 공유 판단을 따로 구현하면 정책이 달라지고 권한 회수 시점이 불명확해진다. Relation Guard는 관계 쓰기와 권한 판정을 하나의 계약으로 제공한다. 외부 앱의 DB schema를 강제하지 않는다.

제품 대상은 같은 운영 조직이 신뢰하는 백엔드 서비스다. 인터넷의 임의 사용자에게 직접 공개하는 멀티테넌트 SaaS까지 만들지 않는다. `tenant`는 고객 조직 경계이며 운영자가 사전 생성한다.

## MVP 포함

| 항목 | 구체적 범위 | 완료 증거 |
|---|---|---|
| 객체 등록/삭제 | user, team, folder, document의 최소 ID registry | 존재 검증, 삭제 제한 테스트 |
| 관계 변경 | member, parent, viewer, editor 관계의 원자적 batch write | 실패 시 전부 rollback |
| 단건 판단 | user가 folder/document에 view/edit 가능한지 | 규칙별 allow/deny fixture |
| 일괄 판단 | 같은 사용자에 대해 최대 100개 check | 단일 snapshot, 입력 순서 유지 |
| 설명 | allow를 뒷받침하는 유효 경로 하나 | explain scope 및 경로 검증 |
| 격리 | 요청, query, FK 모두 tenant 포함 | 다른 tenant의 동일 ID 테스트 |
| 일관성 | primary snapshot, tenant revision, idempotency | 회수·재시도·충돌 스케줄 테스트 |
| 운영 | health/readiness, 구조화 로그, 제한, graceful shutdown | DB 장애 및 과부하 테스트 |
| 연동 예제 | 별도 Python 표준 라이브러리 클라이언트와 테스트 시나리오 | 다른 프로세스에서 grant→check→revoke→check |

관리 보조 기능인 관계 목록·revision·변경 감사 목록도 제공한다. 이는 정책 관리에 필요한 조회이며 모든 접근 가능한 문서를 찾아주는 검색 엔진은 아니다.

## 의도적으로 제외

- 로그인, OAuth/OIDC 서버, 비밀번호, 최종 사용자 세션, 원본 문서 저장.
- role editor UI, 결제, 조직 초대 UI, 공유 링크, 자동 만료 권한.
- ABAC 조건식, 명시적 deny, 소유자 자동 우회, 그룹 중첩, 다중 부모.
- 임의 관계 schema/DSL, Cedar·Zanzibar·SpiceDB·OpenFGA API 호환성.
- 모든 접근 가능 리소스 조회(ListObjects), 페이지네이션 전역 최적화.
- cross-DB atomicity, 전역 선형화, 인플라이트 수정의 즉시 취소.
- Redis/분산 캐시, Kafka, CDC, gRPC, K8s/sidecar 배포 자동화, multi-region.
- 불필요한 기존 realtime-support-platform 코드 이식. 독립 저장소로 시작한다.

## 성공 정의

M0–M5를 마치고 모든 보안/정확성 gate가 통과해야 v0.1 complete다. API가 200을 반환하거나 데모 한 번 성공한 것으로 완료하지 않는다. 숫자 성능 목표는 검증 계획이며 달성 전 홍보 문구에 사용하지 않는다.

배포 가능한 포트폴리오 수준과 보안 감사된 상용 제품 수준은 다르다. 결과물은 보장 범위와 한계를 함께 설명한다. Rust 사용 자체나 범용 제품과의 기능 개수 경쟁을 성공 기준으로 삼지 않는다.

## 주요 리스크

1. Rust 학습 부담: 먼저 core의 작은 규칙부터 구현하고 네트워크/DB를 나중에 연결.
2. 관계 원본의 분산: v0.1에서 엔진을 권한 관계의 단일 쓰기 지점으로 정함.
3. 데이터가 커질수록 탐색 비용 증가: 명시적 한도, batch query, 재현 가능한 profile로 대응.
4. tenant write 직렬화: 단순성과 정확성의 대가. 동시 write 처리량을 측정하고 결과를 공개.
5. 권한 변경과 앱 작업 사이 경쟁: API만으로 해결됐다고 주장하지 않고 통합 계약에 명시.
