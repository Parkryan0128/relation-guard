# 09. 테스트 단계

현재 구현은 `org-doc-v2`다. 기능 목록은 [범위](01-scope.md), 사용자별 변경 권한은 [관리 계약](15-managed-authorization.md), 실제 통과 여부는 [검증 현황](../verification/status.md)을 따른다.

## 실행과 기준

`scripts/verify.sh`는 fmt, Clippy, 계약 검사, Rust/실제 PostgreSQL 테스트, HTTP·COMMIT 장애·클라이언트 검사를 실행한다. [실행 가이드](13-running.md)에서 테스트 DB와 Python 의존성을 먼저 준비한다.

`scripts/test-stage.sh`는 아래 단계를 누적 실행한다. M1부터 core benchmark를 기록하고, M4는 native 복구, M5는 보안·부하 검사를 추가한다. Compose 배포는 별도 검증이다.

| 단계 | 명령 | 필수 검증 | 초기 증거 |
|---|---|---|---|
| M0 계약 | `scripts/test-stage.sh M0` | DTO·OpenAPI, ID/JSON 경계, migration·DB 제약. M0-01–05 | [M0](../verification/M0-report.md) |
| M1 평가기 | `scripts/test-stage.sh M1` | 직접·팀·상속 권한, proof, 한도, 독립 평가기 비교 10,000회. M1-01–06 | [M1](../verification/M1-report.md) |
| M2 저장 | `scripts/test-stage.sh M2` | rollback, revision·receipt·audit, 동시 변경, snapshot, batch SELECT ≤3. M2-01–08 | [M2](../verification/M2-report.md) |
| M3 HTTP | `scripts/test-stage.sh M3` | 인증·scope·tenant 격리, 오류 계약, timeout·overload·COMMIT 장애. M3-01–07 | [M3](../verification/M3-report.md) |
| M4 통합 | `scripts/test-stage.sh M4` | Python 연동, 재시작·재연결, backup/restore, 키 교체, 종료·풀 복구. M4-01–06 | [M4](../verification/M4-report.md) |
| M5 부하 | `scripts/test-stage.sh M5` | 활성 의존성 보안 검사, S 단건/배치 각 3회, M·mixed·stress·복구. M5-01–05 | [M5](../verification/M5-report.md) |

세부 입력과 기대 결과는 [테스트 매트릭스](12-acceptance-tests.md)를 따른다. 초기 증거는 당시 결과이며 현재 버전의 통과 증거로 대체하지 않는다. V1 관리 권한 검사는 core/store/HTTP 회귀 검사에 포함된다.

## 판정

- 허용·거부뿐 아니라 tenant 격리, 권한 없는 변경, 자기 승격, 마지막 관리자 보호, 중복 요청, 동시성도 검사한다.
- 변경 실패 시 graph·revision·audit·receipt가 모두 유지되어야 한다. 의도한 장애는 지정된 오류와 이후 복구까지 확인한다.
- 동시성은 barrier로 순서를 제어한다. 반복 횟수와 경계값은 매트릭스를 따르며 실패를 재시도로 숨기지 않는다.
- 부하 기준은 [측정 방법](08-validation.md)을 따른다. 실패·skip·환경 차이를 통과로 표현하지 않는다. Native 성능과 컨테이너 배포 결과는 구분한다.
- 증거에는 commit, 환경, 명령, PASS/FAIL/SKIP, 오류 수와 측정치를 남긴다. 비밀값은 제외한다. 이전 실패 기록은 보존한다.

복구·부하 검사의 도구와 환경 변수는 [재현 명령](14-native-verification.md)에 있다. 이후 기능은 실제 요구가 있을 때 별도 범위로 정한다.
