# 13. 현재 구현 실행과 검증

로컬 실행과 Compose 설정을 설명한다. 현재 테스트 결과와 환경 제한은 [검증 현황](../verification/status.md)에 있다.

## 로컬 바이너리

Rust는 rust-toolchain.toml, 의존성은 Cargo.lock으로 고정한다. PostgreSQL 18의 별도 DB를 준비한다. 실서비스 DB로 테스트하지 않는다.

```bash
cargo build --locked --workspace
export DATABASE_URL='postgres://postgres@127.0.0.1:5432/relation_guard'
cargo run --locked -p authz-admin -- migrate
cargo run --locked -p authz-admin -- tenant 11111111-1111-4111-8111-111111111111
cargo run --locked -p authz-admin -- bootstrap-admin 11111111-1111-4111-8111-111111111111 local-admin
cargo run --locked -p authz-admin -- key 11111111-1111-4111-8111-111111111111 local
```

`key` 출력의 token은 클라이언트용 secret이다. 출력의 config_entry를 `{ "version": 1, "keys": [ ... ] }`로 감싸 private 파일에 저장한다. 토큰을 Git·로그에 올리지 않는다. 예제 key는 전체 scope라서 운영 사용 시 각 클라이언트의 최소 scope로 줄인다. 서버는 시작 시 파일을 읽으며 key rotation은 새/옛 키를 함께 배포한 뒤 옛 키를 제거하고 재시작하는 방식이다.

```bash
# 관리자 DATABASE_URL 상태에서 runtime 전용 role을 만든다.
export AUTHZ_RUNTIME_PASSWORD="$(openssl rand -hex 32)"
cargo run --locked -p authz-admin -- runtime-role
# 앱에는 관리자 URL을 넘기지 않는다. 생성한 runtime 비밀번호를 사용한다.
export DATABASE_URL="postgres://relation_guard_runtime:${AUTHZ_RUNTIME_PASSWORD}@127.0.0.1:5432/relation_guard"
unset AUTHZ_RUNTIME_PASSWORD
export AUTHZ_KEYS_FILE=/absolute/private/path/keys.json
export AUTHZ_BIND=127.0.0.1:8088
export AUTHZ_OPS_BIND=127.0.0.1:9098
cargo run --locked -p authz-server
```

키 파일은 소유자만 읽을 수 있도록 `chmod 600`으로 저장한다. 관리 명령(migrate/tenant/runtime-role/cleanup-receipts)을 실행할 때는 별도 관리자 DB 자격 증명을 사용한다. runtime role은 tenant 생성과 audit 삭제 권한이 없다.

운영 listener의 `/healthz`, `/readyz`, `/metrics`와 데이터 listener를 분리한다. 기본 bind는 loopback이다. 데이터 port만 인증된 서비스 네트워크에 연결한다. 외부 HTTPS는 reverse proxy에서 종료한다. 이 구현이 TLS 서버 또는 end-user 로그인 서버는 아니다.

## Docker Compose

```bash
python3 scripts/bootstrap-local.py
docker compose build
docker compose run --rm migrate
# bootstrap 출력의 tenant UUID 사용
docker compose run --rm --entrypoint authz-admin migrate tenant TENANT_UUID
docker compose run --rm --entrypoint authz-admin migrate bootstrap-admin TENANT_UUID local-admin
docker compose up -d server
python3 examples/client.py
```

다른 앱이 8080을 사용하면 `.env`의 AUTHZ_PORT를 빈 port로 바꾸고 client에 AUTHZ_URL을 지정한다. 초기화 스크립트는 기존 `.env` 또는 secrets 디렉터리가 있으면 중단하며 덮어쓰지 않는다. DB와 metrics에는 host port 매핑을 두지 않았다. Compose `.env`와 secrets 디렉터리는 Git에서 제외한다. 단일 개발용 Compose에서 DB URL은 컨테이너 환경 변수로 제공되므로 Docker 접근 권한이 있는 운영자는 이를 읽을 수 있다.

## 테스트

```bash
python3 -m venv .venv
.venv/bin/pip install -r scripts/requirements-test.txt
# DB 관리자가 pg_stat_statements를 shared_preload_libraries에 추가하고 재시작해야 한다.
export TEST_DATABASE_URL='postgres://postgres@127.0.0.1:5432/relation_guard_test'
export PYTHON="$PWD/.venv/bin/python"
scripts/verify.sh
scripts/test-stage.sh M0
scripts/test-stage.sh M1
cargo bench --locked -p authz-core --bench evaluation
```

`verify.sh`는 현재 구현된 회귀 검사 전체를 실행한다. stage gate와 다르다. stage script는 M0–M3를 누적 실행하고, M4부터는 사용자 승인된 native 검증 경로를 사용한다. 실패를 PASS로 바꾸지 않는다. 테스트는 고유 schema 또는 tenant를 사용하지만 테스트 DB에 데이터를 남기므로 **일회용 DB 사용 후 DB 전체를 폐기**한다. test DB URL을 운영 DB URL로 설정하지 않는다. proptest 기본 10,000 cases, seed 424242. 실패를 재시도로 숨기지 않는다.

## 운영 CLI

`authz-admin cleanup-receipts`는 24시간 초과 receipt를 한 번에 최대 1,000개 삭제한다. audit과 graph revision은 바꾸지 않는다. 자동 스케줄러는 포함하지 않는다. receipt 정리 이후에는 과거 성공 응답이 재생되지 않을 수 있으므로 expected_revision 충돌을 올바르게 처리해야 한다.

백업·복구와 key rotation은 docs/07에 있는 운영 계획을 따른다. 별도 DB에 대한 native restore 실험은 통과했다. 컨테이너 볼륨/배포 검증은 별도이며 프로덕션 준비 완료를 주장하지 않는다.

## org-doc-v1에서 전환

1. 기존 writer와 구버전 서버를 멈추고 DB를 백업한다. 서로 다른 모델의 서버를 섞어서 실행하지 않는다.
2. 운영자 자격으로 migrate를 실행한다. 기존 사용자는 자동으로 admin이 되지 않는다.
3. 각 조직에 bootstrap-admin UUID USER_ID로 실제 관리자를 지정한다. revision과 감사 기록이 증가한다.
4. 모든 호출에 검증된 로그인 사용자 X-Actor-Id를 추가하고 org-doc-v2 응답을 처리하도록 클라이언트를 갱신한다. Python Client의 네 번째 인자는 actor다. client.json도 actor_id가 필요하다.
5. 새 서버를 시작하고 관리자/일반 사용자 허용·거부를 확인한 뒤 트래픽을 연다. 구버전으로 단순 바이너리 롤백하지 않는다. 필요하면 백업으로 별도 복구한다.

bootstrap-admin은 기존 admin이 있으면 실패한다. 긴급 복구는 privileged DB 자격으로 recover-admin UUID USER_ID를 사용하며 감사 근거에 operator_recovery가 남는다. 일반 HTTP에는 이 경로가 없다.

## 관리 권한 예제

`examples/client.py`는 private client.json의 조직 관리자 actor로 사용자 두 명과 문서를 만든다. 그 뒤 문서 관리자 actor가 공유하고, editor actor의 자기 승격은 거부되며, 관리자가 회수하면 editor의 접근이 거부된다. 실제 앱은 예제처럼 임의로 actor를 선택하지 말고 검증된 로그인 세션에서 가져온다.

macOS에서 Documents의 secret 파일을 마운트하지 못하면 시스템 설정 → 개인정보 보호 및 보안 → 파일 및 폴더에서 Docker의 Documents 접근을 확인한다.
