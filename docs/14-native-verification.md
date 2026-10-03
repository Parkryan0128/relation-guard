# 14. 복구·부하 검증

앱과 테스트를 호스트에서 실행하고 전용 PostgreSQL에 연결한다. DB는 Docker에서도 실행할 수 있다. Native 결과와 Compose 배포 결과는 구분하며, Linux 컨테이너 자원 제한 성능을 대신하지 않는다. 현재 결과와 이전 실패는 [검증 현황](../verification/status.md)에 있다.

## 재현

PostgreSQL 18에 `shared_preload_libraries='pg_stat_statements'`를 설정하고 재시작한다. 테스트 DB는 기존 앱 데이터가 없는 전용 인스턴스여야 한다. 로컬에는 PostgreSQL 18 클라이언트 도구(psql/pg_dump/pg_restore), Rust, Python 및 k6가 필요하다. 서버 실행에는 Docker가 필요 없다.

```bash
export TEST_DATABASE_URL=postgres://postgres@127.0.0.1:5432/relation_guard_test
export EXISTING_TEST_DATABASE_URL="$TEST_DATABASE_URL"
export PG_BIN=/absolute/path/to/postgresql/bin
export PYTHON=/absolute/path/to/python-with-jsonschema
scripts/test-stage.sh M4
```

기본 store 테스트는 고유 schema/tenant를 사용하고 일부 데이터를 남긴다. recovery/load 테스트는 고유 database를 생성하고 종료 시 해당 DB를 삭제한다. `TEST_DATABASE_URL`을 실서비스 DB로 설정하지 않는다.

M5-native는 시간이 오래 걸린다. 단건·batch 각각 60초 예열+300초 측정 3회, 이후 M/mixed/stress/recovery를 실행한다.

```bash
cargo build --release --locked -p authz-server -p authz-admin
export AUTHZ_BIN="$PWD/target/release"
export K6_BIN=/absolute/path/to/k6
export RG_LOAD_DIR=/absolute/private/scratch/directory
export RG_LOAD_OUTPUT=/absolute/path/to/new-results
python3 scripts/compare_batch.py
python3 scripts/run_native_load.py
```

`compare_batch.py`는 S 데이터에서 100개 판정을 순차 단건·10개 동시 단건·배치 1회로 실행하는 별도 소규모 비교다. Python client scheduling이 포함되며 arrival-rate 부하/SLO 결과와 섞지 않는다. 본 부하 테스트와 동시에 실행하지 않는다.

`RG_LOAD_DIR`에는 테스트 토큰이 있으므로 Git에 넣지 않는다. `RG_LOAD_OUTPUT`에는 토큰 없는 통계를 저장한다. 기존 증거를 덮어쓰지 않도록 새 디렉터리를 지정한다. 결과에는 요청 수·실패·미시작 iteration·정확성 불일치·백분위수·애플리케이션 내부 카운터와 실행 환경을 포함한다. CPU/RSS는 자식 서버 종료 시 부모의 wait4로 측정한다.


## 의존성 검토 재현

RustSec DB의 최신 revision을 기록한 JSON을 만든 뒤 실제 활성 dependency closure를 검토한다. 현재 lockfile의 optional SQLx/MySQL 의존성에는 rsa advisory가 있으므로 raw audit의 nonzero exit를 무조건 무시하면 안 된다. `check_security.py`는 server/admin의 macOS arm64 및 Linux arm64/x86_64 활성 의존성에 advisory가 있으면 실패한다. 지원 target 또는 feature를 바꾸면 다시 검토한다.

```bash
cargo install --locked cargo-audit --version 0.22.2
# finding이 있으면 nonzero exit와 JSON을 함께 검토한다.
cargo audit --json > /absolute/private/scratch/audit.json
export AUDIT_JSON=/absolute/private/scratch/audit.json
python3 scripts/check_security.py
```

`verification/security-review.json`에 advisory DB revision, finding 처리 이유, 활성 라이선스 목록과 tracked 파일 secret-pattern 검토 결과를 기록한다. 패턴 검사는 모든 secret 부재의 증명이 아니다.
