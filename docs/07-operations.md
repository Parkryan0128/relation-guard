# 07. 보안·배포·운영

## 신뢰 경계

end user → 기존 앱 → Relation Guard → PostgreSQL. 사용자 인증과 subject 선택 책임은 기존 앱에 있다. 사용자가 입력한 user ID를 그대로 믿고 엔진에 넘기지 않는다. Relation Guard는 신뢰된 caller의 tenant와 scope를 검증한다.

v0.1 서비스 인증: CSPRNG로 생성한 정확히 32바이트 secret, key_id, 단일 tenant UUID, scopes를 운영자 관리 secret 파일에 배포한다. token은 `key_id.secret` 형태이고 저장 설정에는 secret SHA-256 digest만 둔다. 고엔트로피 secret용이며 사용자 password hashing 설계가 아니다. digest는 constant-time 비교, 비밀값은 로그/CLI 인자/URL에 노출하지 않는다. 원문은 최초 생성 시 안전한 secret 전달 경로로만 제공한다.

credential secret 파일은 read-only mount, app DB에는 넣지 않는다. key rotation은 새·옛 key 잠시 중복 배포 후 옛 key 제거 및 모든 replica 재시작으로 처리한다. v0.1은 hot reload/즉시 분산 credential 회수를 약속하지 않는다. startup 시 잘못된 tenant/scope/key 중복 설정은 fail-fast. admin CLI는 별도 privileged DB credential로 tenant와 초기 revision 0을 생성하며 runtime 서버에 그 credential을 제공하지 않는다.

caller scope: check, explain, relations:read, relations:write, audit:read. writer는 해당 tenant의 모든 권한 관계를 관리하는 신뢰된 서비스다. 개별 최종 사용자가 grant할 자격이 있는지는 기존 앱이 검사해야 하며, check-only key를 writer로 승격할 수 없다.

## 실패 정책

- 보호 작업은 명시적인 정상 allow에서만 수행. deny와 error는 구분하되 둘 다 작업을 실행하지 않는다.
- DB 중단·pool 포화·손상·deadline에 fail-open하지 않는다.
- SQL bind, strict DTO, 크기/깊이/작업 제한, 중앙 tenant context.
- 설명 경로와 감사 기록은 민감 관계 정보이므로 별도 scope. 기본 응답에는 포함하지 않음.
- log에 토큰·전체 payload·raw subject/resource ID·proof를 남기지 않는다. request ID, key ID, route, decision, bounded error code, duration만 기록. tenant ID metric label 금지.

객체별 권한 검사 누락은 API 보안의 실제 문제다 [R9]. 엔진이 있어도 앱이 호출을 빼먹으면 보호되지 않는다. integration example에서 endpoint별 check 위치를 보여준다.

## 실행과 네트워크

개발: Docker Compose의 authz + db, 선택적 example-client. DB는 내부 네트워크만, authz 개발 port도 host 127.0.0.1에만 publish. 운영: authz와 앱 별도 컨테이너, private network, TLS 종료 proxy 또는 플랫폼 service TLS. 내부 네트워크만으로 인증/암호화를 대체했다고 주장하지 않는다. DB 원격 연결은 TLS 검증한다.

이미지는 builder에서 compile 후 non-root runtime에 binary/필수 CA·라이브러리만 복사. 최소 filesystem 권한, 가능한 read-only rootfs. Kubernetes나 sidecar는 필수 아님. Postgres 데이터는 영구 저장소와 backup 대상 [R10,R11].

migration은 배포 전 별도 job/운영 명령으로 수행. schema version 불일치 시 readiness 실패. 요청마다 migration을 시도하지 않는다. future replica를 고려해 startup migration 경쟁을 피한다.

## Health와 관측

- GET /healthz: 프로세스 생존만 확인, DB 장애로 liveness를 실패시키지 않음.
- GET /readyz: drain 아님 + DB 접근 + schema version 확인. timeout 250ms. 내부 health endpoint로 제한, 오류 세부정보 비노출.
- GET /metrics: 내부 운영망만 접근. request count/latency, allow/deny/error, DB/pool wait, evaluator time, facts/work units, write lock wait, conflicts. metric label은 route/status/action 등 제한된 값만.
- SIGTERM: readiness 내려놓고 새 요청 수락 중지, 기존 요청 최대 10초 drain, pool 종료. 강제 종료 시 write commit outcome은 receipt로 복구.

## 복구와 운영 문서 완료 조건

최소 backup→빈 DB restore→revision/relationships/check 재검증 절차를 실제로 한 번 수행하고 결과 기록. WAL/PITR 구성 여부는 배포 환경에 따라 별도 명시. v0.1이 운영 고가용성이나 무손실 failover를 보장하지 않는다.

DB connection budget은 replica 수 × pool size로 계산하고 여유를 admin/migration에 남긴다. single replica in-flight 제한만으로 전역 quota가 구현됐다고 표현하지 않는다. 의존성/라이선스 점검과 secret scan은 릴리스 체크에 포함한다.

## 운영 전 추가 검토

공개 인터넷 노출, 사용자의 직접 API 접근, dynamic API key 관리, 외부 감사 인증, HA/멀티 리전은 별도 설계가 필요하다. 이 항목은 초기 개발을 막는 승인 단계가 아니라 v0.1의 보장 범위를 명시한 것이다.

## credential·listener·shutdown 세부 계약

- key_id는 `[A-Za-z0-9_-]{1,64}`로 dot을 금지한다. secret은 32 random bytes를 unpadded base64url로 인코딩한 43자다. bearer는 `key_id.secret`이며 separator는 정확히 하나. SHA-256은 디코딩한 32 bytes에 적용한다. 형식/unknown key/틀린 secret은 동일한 401 응답을 사용한다.
- credential 파일 형식은 `{version:1,keys:[{key_id,tenant_id,scopes,secret_sha256}]}`. digest는 lowercase hex 64자, scopes는 알려진 문자열의 중복 없는 배열, key_id는 파일 전체에서 유일하다. 존재하지 않는 tenant, empty scope, 중복/알 수 없는 scope는 startup 실패다. key rotation시 새 key_id를 사용하고 삭제한 key_id를 재사용하지 않는다.
- data API는 8080, 운영 listener는 9090. healthz/readyz/metrics는 운영 listener에서만 제공하고 bearer scope 체계와 섞지 않는다. Compose는 9090을 host에 publish하지 않는다. 운영 환경도 network policy로 제한한다.
- healthz: 살아 있으면 200 `{status:"ok"}`. readyz: 준비 시 200 `{status:"ready"}`, DB/schema/drain 실패 시 503 `{status:"not_ready"}`. metrics는 Prometheus text. 운영 응답은 data API의 공통 JSON envelope에서 제외한다.
- shutdown은 readiness false → data listener 신규 수락 중단 → in-flight drain 최대 10초 → pool 종료. 10초 안에 종료 못 한 연결은 닫고 로그에 남긴다. commit 상태는 별도로 확정하거나 unknown으로 남기며 허위 rollback 기록을 하지 않는다.
- in-flight 제한은 instance별 전체 data API에 적용하고 대기열을 만들지 않는다. 128개 점유 시 새 요청은 503 OVERLOADED. 운영 listener는 별도의 작은 capacity를 확보한다. 단일 tenant가 전체 capacity를 쓸 수 있다는 한계는 남으며 tenant별 quota는 v0.1 범위 밖이다.
