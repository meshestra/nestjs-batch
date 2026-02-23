# Ecommerce Settlement Batch Example

`@nestjs-batch` 프레임워크를 활용한 이커머스 정산 배치 서버 예제입니다.

---

## Features

| 기능 | 설명 |
|------|------|
| 일별 주문 정산 | 전일 완료 주문을 집계하여 판매자별 정산 금액 산출 |
| 정산 실패 재처리 | 실패한 정산 건을 감지하여 재시도 처리 |
| 포인트/쿠폰 만료 | 유효기간이 지난 포인트·쿠폰을 일괄 만료 처리 |
| 휴면 계정 전환 | 장기 미접속 사용자를 휴면 상태로 전환 |

---

## Scenarios

### 1. 일별 주문 정산 (`DailySettlementJob`)

매일 새벽 2시, 전날 `DELIVERED` 상태로 완료된 주문을 판매자(seller) 단위로 집계합니다.

```
[OrderReader]
  └─ 전일 완료 주문 chunk 단위 조회 (chunkSize: 500)
      ↓
[SettlementProcessor]
  └─ 주문별 수수료 계산, 취소/환불 차감
      ↓
[SettlementWriter]
  └─ seller_settlements 테이블에 upsert
      └─ 실패 시 retryLimit: 3 재시도
```

**Job Parameters**
- `targetDate` : 정산 대상 날짜 (기본값: 어제)

---

### 2. 정산 실패 재처리 (`SettlementRetryJob`)

매일 새벽 4시, `FAILED` 상태의 정산 건을 다시 처리합니다.

```
[FailedSettlementReader]
  └─ status = FAILED 인 정산 레코드 조회
      ↓
[SettlementRetryProcessor]
  └─ 실패 원인 검증, 재정산 금액 계산
      ↓
[SettlementRetryWriter]
  └─ 정산 상태 업데이트 (FAILED → COMPLETED)
      └─ skipLimit: 10 — 재실패 건은 skip 후 알림
```

---

### 3. 포인트/쿠폰 만료 처리 (`ExpireRewardsJob`)

매일 자정, 당일 만료되는 포인트와 쿠폰을 일괄 처리합니다.

```
Step 1 — ExpirePointsStep
  [PointReader] → [ExpireProcessor] → [PointWriter]
  └─ 만료 포인트 잔액 차감 및 이력 기록

Step 2 — ExpireCouponsStep
  [CouponReader] → [ExpireProcessor] → [CouponWriter]
  └─ 쿠폰 상태 EXPIRED 로 업데이트
```

---

### 4. 휴면 계정 전환 (`DormantAccountJob`)

매월 1일 새벽 1시, 최근 12개월간 로그인 이력이 없는 사용자를 휴면 처리합니다.

```
[UserReader]
  └─ last_login_at < 12개월 전, status = ACTIVE 인 사용자 조회
      ↓
[DormantProcessor]
  └─ 개인정보 마스킹, 휴면 전환 이벤트 생성
      ↓
[DormantWriter]
  └─ 사용자 상태 ACTIVE → DORMANT 업데이트
      └─ 알림 이벤트 발행
```

---

## Domain Model

```
orders              — 주문 (order_id, seller_id, status, total_amount, delivered_at)
order_items         — 주문 상품
sellers             — 판매자
seller_settlements  — 판매자 정산 (settlement_date, amount, fee, status)
points              — 포인트 잔액 (user_id, balance, expires_at)
point_histories     — 포인트 이력
coupons             — 쿠폰 (user_id, status, expires_at)
users               — 사용자 (status, last_login_at)
```

---

## Batch Framework Concepts Used

| 개념 | 적용 |
|------|------|
| `@Job` / `@Step` | 각 정산 잡 클래스와 스텝 메서드 선언 |
| `ItemReader` | 대용량 주문·사용자 데이터 chunk 단위 페이징 조회 |
| `ItemProcessor` | 수수료 계산, 마스킹 등 비즈니스 로직 처리 |
| `ItemWriter` | DB upsert / 상태 업데이트 |
| `chunkSize` | 500건 단위 트랜잭션으로 메모리 안정성 확보 |
| `retryLimit` | Writer 실패 시 최대 3회 재시도 |
| `skipLimit` | 재처리 불가 건 skip 후 계속 진행 |
| `preventDuplicateRun` | 동일 잡 중복 실행 방지 |
| `JobParameters` | `targetDate` 등 런타임 파라미터 전달 |
