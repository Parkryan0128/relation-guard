# 13. 현재 구현 실행과 검증

현재 core/store/HTTP/admin 구현은 로컬 PostgreSQL과 TCP 테스트를 통과했다. **전체 M0–M5 release gate 통과를 의미하지 않는다.** 복구와 COMMIT ack 유실 검증은 통과했다. 현재 native 검증 계획은 docs/14-native-verification.md에 있다. 배포·부하 검증의 상태는 [검증 현황](../verification/status.md)을 따른다.

## 로컬 바이너리

Rust는 rust-toolchain.toml, 의존성은 Cargo.lock으로 고정한다. PostgreSQL 18의 별도 DB를 준비한다. 실서비스 DB로 테스트하지 않는다.

```bash
cargo build --locked --workspace
export DATABASE_URL='postgres://postgres@127.0.0.1:5432/relation_guard'
cargo run --locked -p authz-admin -- migrate
cargo run --locked -p authz-admin -- tenant 11111111-1111-4111-8111-111111111111
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

## Compose (이미지 빌드 성공, clean deployment 검증은 별도)

```bash
python3 scripts/bootstrap-local.py
docker compose run --rm migrate
# bootstrap 출력의 tenant UUID 사용
docker compose run --rm --entrypoint authz-admin migrate tenant TENANT_UUID
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
PYTHON=.venv/bin/python scripts/verify.sh
scripts/test-stage.sh M0
scripts/test-stage.sh M1
cargo bench --locked -p authz-core --bench evaluation
```

`verify.sh`는 현재 구현된 회귀 검사 전체를 실행한다. stage gate와 다르다. stage script는 M0–M3를 누적 실행하고, M4부터는 사용자 승인된 native 검증 경로를 사용한다. 실패를 PASS로 바꾸지 않는다. 테스트는 고유 schema 또는 tenant를 사용하지만 테스트 DB에 데이터를 남기므로 **일회용 DB 사용 후 DB 전체를 폐기**한다. test DB URL을 운영 DB URL로 설정하지 않는다. proptest 기본 10,000 cases, seed 424242. 실패를 재시도로 숨기지 않는다.

## 운영 CLI

`authz-admin cleanup-receipts`는 24시간 초과 receipt를 한 번에 최대 1,000개 삭제한다. audit과 graph revision은 바꾸지 않는다. 자동 스케줄러는 포함하지 않는다. receipt 정리 이후에는 과거 성공 응답이 재생되지 않을 수 있으므로 expected_revision 충돌을 올바르게 처리해야 한다.

백업·복구와 key rotation은 docs/07에 있는 운영 계획을 따른다. 별도 DB에 대한 native restore 실험은 통과했다. 컨테이너 볼륨/배포 검증은 별도이며 프로덕션 준비 완료를 주장하지 않는다.
