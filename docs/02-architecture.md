# 02. 스택과 구조

> 현재 확장 계약: [사용자 기준 권한 관리](15-managed-authorization.md). 아래 v0.1 설명 중 신뢰된 writer의 무조건 변경, view/edit만 지원, actor 없는 감사/receipt 규칙은 이 확장에서 대체된다.
## 선택

| 계층 | 선택 | 선택 이유 / 대가 |
|---|---|---|
| 언어 | Rust stable, edition 2024 | 새 생태계 학습 + 안전한 자료구조/동시성. Java/Go/C++보다 빠르다고 가정하지 않음 |
| HTTP | Axum | 명시적 handler/extractor, Tower middleware, Tokio 통합 [R1] |
| 실행 | Tokio | 비동기 socket·DB I/O, timeout·shutdown [R2] |
| JSON | Serde + serde_json | 타입별 DTO, unknown field 거부, 명시적 error mapping |
| SQL | SQLx | async pool·transaction·명시적 SQL, ORM의 암묵적 query 회피 [R3] |
| 저장소 | PostgreSQL 18 계열의 검증된 patch | FK, transaction, recursive CTE. MySQL도 대안이나 v0.1은 단일 DB만 지원 [R4,R5] |
| 스키마 변경 | SQLx migrations | 애플리케이션과 같은 toolchain. 서버 시작마다 자동 migration 실행하지 않음 |
| 관측 | tracing + HTTP metrics | request/DB/evaluator 시간 분리. PII 없는 저 cardinality metric |
| 검증 | cargo test, proptest, 실제 PostgreSQL | 모델 속성 검사 및 SQL 의미 검증 [R6] |
| 성능 | Criterion(core), k6(HTTP) | 계산 비용과 네트워크 포함 비용 분리 |
| 배포 | multi-stage Docker, Compose | 별도 app/authz/db 컨테이너. 초기 Kubernetes 불필요 |
| CI | GitHub Actions | fmt/lint/unit/integration/contract gates |

R 번호의 원문은 [근거 자료](11-references.md). crate의 정확한 patch와 MSRV는 M0에서 호환성을 확인하고 rust-toolchain.toml, Cargo.lock에 고정한다. 이 문서의 제품 선택을 검증 안 된 최신 버전 pin으로 바꾸지 않는다. PostgreSQL image도 검증된 patch/digest로 고정한다.

## 구현된 workspace

```text
crates/
  authz-core/       # IDs, typed edges, semantics, pure evaluator, proof
  authz-store/      # SQLx queries, migrations, snapshot fact loader, write transactions
  authz-server/     # Axum routes, auth, DTO, limits, errors, orchestration
  authz-admin/      # trusted local tenant provisioning; no public admin API
migrations/
examples/client.py # small HTTP caller, not another business service
benchmarks/        # seeded data, load profiles, reports
```

crate 내부에서도 책임을 나눈다. core의 `contract.rs`는 요청/응답 계약, `json.rs`는 중복 키를 거부하는 파서를 담당한다. store의 `database.rs`는 연결/트랜잭션 시작, `read.rs`는 snapshot 조회/평가, `write.rs`는 변경/receipt/audit, `management.rs`는 변경 권한 규칙, `error.rs`는 오류 변환을 담당한다. server의 `main.rs`는 프로세스/운영 endpoint, `api.rs`는 데이터 API를 담당한다. 모듈 분리는 SQL, 권한 규칙, timeout 값을 바꾸지 않는다.

위 계층은 현재 구현돼 있다. Python 클라이언트는 examples/client.py에 있다. 핵심 의존 방향은 store → core, server → core/store다. core는 HTTP/SQL/runtime에 의존하지 않는다. 저장소가 같은 snapshot에서 조회한 제한된 facts를 core에 넘긴다. core는 결정과 증명 경로를 계산한다.

## 배포 경계

기존 앱은 인증된 end-user ID를 가져와 자신의 credential로 authz를 호출한다. authz가 로그인 여부를 대신 판단하지 않는다. 앱의 credential과 사용자의 subject ID를 혼동하지 않는다.

authz는 전용 PostgreSQL primary를 사용한다. 개발 Compose에서는 DB 컨테이너 + 영구 volume, 실제 운영에서는 같은 네트워크의 관리형 PostgreSQL도 가능하다. 같은 컨테이너에 앱·authz·DB를 모두 넣지 않는다. 권한 DB는 다른 앱이 직접 쓰지 않는다.

## 초기 성능 방침

- 요청 간 cache 없이 시작. 요청 안에서는 ancestor/fact를 deduplicate한다.
- batch 전체에서 bounded set-based SQL을 사용. 리소스마다 DB round trip을 만들지 않는다.
- async는 CPU 계산을 자동 병렬화하지 않는다. 큰 계산은 limit로 차단하고 profile 후 별도 bounded CPU pool 필요성을 판단한다 [R2].
- 서버 replica 증가는 나중에도 primary/tenant lock 계약을 유지해야 한다. replica별 독립 SQLite나 독립 관계 DB는 채택하지 않는다.
- HTTP 먼저. gRPC는 측정/클라이언트 요구가 생기면 별도 ADR로 추가한다.
