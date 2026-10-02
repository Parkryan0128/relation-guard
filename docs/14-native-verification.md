# 14. 검증 환경 변경 기록 — 2026-10-02

사용자는 전체 구현을 계속하되, Docker 배포 테스트가 로컬 권한 문제로 기능 검증을 막지 않도록 **앱/테스트를 로컬에서 실행**하라고 지시했다. 이어 기존 전용 테스트 PostgreSQL에 연결하는 방식을 명시적으로 승인했다.

이 요청을 반영해 이번 완료 범위를 다음처럼 구분한다.

- M0–M3: 원래 기능/트랜잭션/보안 계약과 필수 테스트를 유지한다. 실제 PostgreSQL, 실제 TCP, DB wire proxy를 사용한다.
- M4-native: 로컬 앱 프로세스의 비-root 실행, 별도 runtime DB role, Python 클라이언트, 앱 재시작과 DB 재연결, 빈 DB로 backup/restore, 키 교체, DB 연결 차단·스키마 불일치·SIGTERM을 검증한다. 새 DB는 기존 테스트 PG 안에 고유 이름으로 생성하고 끝나면 해당 DB만 삭제한다.
- M5-native: 원래 S/M 데이터와 부하·정확성 기준을 유지하되 로컬 네이티브 앱+기존 테스트 PG 연결에서 측정한다. Linux의 2 vCPU/512 MiB 앱·2 vCPU/1 GiB DB 제한을 적용한 결과로 표현하지 않는다. CPU/메모리 상한, 컨테이너 네트워크, CPU 모델에 대한 보장이 다르다.
- Docker 배포 검증: 파일과 이미지 빌드는 준비했으나 bind mount 접근 제한으로 clean Compose 시나리오를 완료하지 못했다. 사용자가 배포 작업을 재개할 때 원래 M4 및 Linux M5 조건을 별도로 확인한다. native PASS를 container PASS로 바꾸지 않는다.

이는 권한 모델 또는 API 일관성 계약 변경이 아니다. `org-doc-v1`은 유지한다. 숫자 목표를 낮추지 않고 **측정 환경과 완료 범위를 명시적으로 분리**한다. native 결과가 목표를 넘으면 그 결과를 그대로 기록한다.

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
python3 scripts/compare_batch.py
python3 scripts/run_native_load.py
```

`compare_batch.py`는 S 데이터에서 100개 판정을 순차 단건·10개 동시 단건·배치 1회로 실행하는 별도 소규모 비교다. Python client scheduling이 포함되며 arrival-rate 부하/SLO 결과와 섞지 않는다. 본 부하 테스트와 동시에 실행하지 않는다.

`RG_LOAD_DIR`에는 테스트 토큰이 있으므로 Git에 넣지 않는다. `verification/load`에는 토큰 없는 통계만 저장한다. 결과에는 요청 수·실패·미시작 iteration·정확성 불일치·백분위수·애플리케이션 내부 카운터와 실행 환경을 포함한다. macOS sandbox에서 다른 프로세스 목록을 읽는 `ps`는 거절되었으므로 전체 호스트 프로세스를 스캔하지 않는다. 자식 서버 종료 시 부모의 wait4로 얻은 해당 서버의 CPU/RSS 사용량만 기록한다.


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
