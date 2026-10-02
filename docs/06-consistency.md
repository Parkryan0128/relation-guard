# 06. 일관성과 외부 앱 연동

## v0.1의 보장

같은 정상 primary DB를 사용하고 failover/data loss가 없다는 전제에서:

- write 성공은 graph/revision/audit/receipt가 commit됐다는 뜻이다.
- 성공 write의 commit 이후 snapshot을 얻은 check는 해당 변경 또는 더 최신 상태를 본다.
- 단건과 batch 각각은 하나의 snapshot을 사용한다. grant와 membership을 서로 다른 시점에서 섞지 않는다.
- 회수와 겹쳐 실행된 이전 snapshot의 check는 기존 allow를 반환할 수 있다. 이미 시작한 앱 작업을 취소하지 않는다.
- 응답 revision은 조회 snapshot의 tenant revision이다. 과거 revision을 지정해 다시 읽는 API, revision 보관 MVCC 서비스, Zanzibar token이 아니다.

이 보장은 PostgreSQL의 snapshot/row lock을 사용하는 제한된 계약이다. 글로벌 external consistency나 여러 DB의 atomicity를 주장하지 않는다 [R4,R7,R8].

## 왜 REPEATABLE READ인가

READ COMMITTED에서 여러 SELECT를 실행하면 각 statement가 다른 시점의 데이터를 볼 수 있다. v0.1은 여러 statement로 facts를 모으므로 요청 전체가 같은 snapshot을 쓰도록 한다. 첫 revision SELECT가 snapshot을 정한다. write는 tenant row lock으로 같은 tenant 변경을 직렬화한다.

## stale retry가 권한을 되살리는 문제

1. grant A가 revision 7로 성공하지만 응답이 유실된다.
2. 다른 요청이 A를 회수해 revision 8이 된다.
3. 첫 요청이 다시 도착한다.

동일 idempotency key라면 receipt를 반환할 뿐 A를 다시 넣지 않는다. receipt는 최소 24시간 보관한다. 이 시간이 지나 receipt가 제거되어도 원래 expected_revision=6은 현재와 불일치하므로 재실행하지 않는다. 그래서 단순 upsert보다 강한 retry 계약을 제공한다.

모든 실제 write가 revision을 증가시키고 revision은 재사용하지 않는다. 복구된 오래된 DB snapshot에서는 revision도 과거로 돌아갈 수 있으므로 재해 복구 시 key를 교체하고 클라이언트 재동기화를 수행한다. 영구 전역 revision이라고 표현하지 않는다.

receipt 보존 정리: 24시간 초과만 trusted admin maintenance에서 batch delete. 삭제는 M2의 운영 CLI 명령으로 수행하며 상주 scheduler는 범위 밖이다. 감사 데이터는 v0.1 자동 삭제하지 않으며 용량을 관측한다.

## 외부 앱 DB와의 경계

권한 관계의 authoritative write path는 이 서비스다. 기존 앱이 membership을 자체 DB에도 보관하면 그 사본은 별도 시스템이며 자동으로 같은 transaction이 되지 않는다.

- 문서 생성: 앱에서 비공개/pending 생성 → 엔진에 entity/relations 동기 write → 성공 확인 후 사용 가능 표시. 실패 시 pending 상태 유지/재시도.
- 권한 회수: 엔진의 회수 성공 후 사용자에게 성공 표시. UI/로컬 사본 갱신 실패는 별도 reconcile 대상.
- 폴더 이동: 앱 표시 계층과 권한 계층을 동시에 바꾸는 전역 transaction은 없다. 불일치 중 동작을 차단하는 앱 상태 전이 또는 별도 coordination이 필요하다.
- async outbox/CDC를 채택하면 반영 지연이 생긴다. v0.1에 Kafka나 자동 동기화를 구현했다고 주장하지 않는다.

클라이언트는 409에서 /revision만 새로 읽고 옛 작업을 무조건 반복하지 않는다. 현재 비즈니스 의도가 여전히 유효한지 재확인한 뒤 새 key로 새 요청을 만든다. 네트워크 retry는 원래 key/payload 그대로 bounded exponential backoff+jitter; 400/401/403/409/422는 자동 retry 금지. 503/504도 자신의 deadline 안에서만 재시도한다.

## check → action 경쟁

```text
T1: check => allow (revision 12)
T2: revoke => committed (revision 13)
T1: 앱 DB의 문서를 수정
```

단순 외부 권한 API는 이 경쟁을 제거하지 못한다. revision을 응답에 넣거나 action 직전 재조회해도 두 DB 사이 gap은 남는다. v0.1은 check 시점의 권한을 제공하며, 수정 commit까지의 권한을 보장하지 않는다. 엄격한 취소 의미가 필요하면 앱과 공유하는 fencing/locking/transaction 설계가 필요하고 후속 범위다.

[근거 자료](11-references.md)의 Zanzibar/SpiceDB는 문제를 이해하기 위한 자료이지 우리 제품이 같은 보장을 가진다는 근거가 아니다.

## 재시도와 restore 계약 확정

예제 client는 최초 시도 포함 최대 3회, 총 8초 예산 안에서 transient failure만 재시도한다. attempt timeout은 최대 2.5초와 남은 예산 중 작은 값, retry delay는 full jitter로 첫 retry 0–100ms, 둘째 0–200ms다. write는 같은 key/정규화 payload를 유지하며 REVISION_EXHAUSTED는 운영 조치가 필요하므로 retry하지 않는다. commit outcome이 불명확한 상태에서 새 key를 만들지 않는다.

재해 복구에서는 traffic을 차단한 상태로 restore하고, 모든 caller credential을 새 key_id/secret으로 교체한 뒤 관계와 caller 의도를 reconcile한다. 이전 revision/receipt/키의 유효성을 보장하지 않는다. 검증된 backup 자체가 오래되었다면 그 이후 revoke도 복구에서 사라질 수 있으므로 상태 확인 없이 traffic을 열지 않는다. 이 서비스는 PITR 이후 자동 권한 복원을 제공하지 않는다.
