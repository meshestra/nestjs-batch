-- ─────────────────────────────────────────────────────────────────────────────
-- 스키마
-- ─────────────────────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS sellers (
  id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  name       VARCHAR(100) NOT NULL,
  fee_rate   NUMERIC(5,4) NOT NULL DEFAULT 0.03,
  created_at TIMESTAMPTZ  NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS orders (
  id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  seller_id     UUID        NOT NULL REFERENCES sellers(id),
  status        VARCHAR(20) NOT NULL,
  total_amount  NUMERIC(12,2) NOT NULL,
  refund_amount NUMERIC(12,2) NOT NULL DEFAULT 0,
  ordered_at    TIMESTAMPTZ NOT NULL,
  delivered_at  TIMESTAMPTZ,
  created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS seller_settlements (
  id                 UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  seller_id          UUID          NOT NULL REFERENCES sellers(id),
  settlement_date    DATE          NOT NULL,
  total_sales_amount NUMERIC(12,2) NOT NULL,
  fee_amount         NUMERIC(12,2) NOT NULL,
  refund_amount      NUMERIC(12,2) NOT NULL DEFAULT 0,
  settlement_amount  NUMERIC(12,2) NOT NULL,
  status             VARCHAR(20)   NOT NULL DEFAULT 'COMPLETED',
  created_at         TIMESTAMPTZ   NOT NULL DEFAULT now(),
  updated_at         TIMESTAMPTZ   NOT NULL DEFAULT now(),
  UNIQUE (seller_id, settlement_date)
);

-- ─────────────────────────────────────────────────────────────────────────────
-- 시드: 판매자 10명
-- ─────────────────────────────────────────────────────────────────────────────

TRUNCATE seller_settlements, orders, sellers RESTART IDENTITY CASCADE;

INSERT INTO sellers (id, name, fee_rate) VALUES
  ('00000000-0000-0000-0000-000000000001', '애플망고마켓',  0.03),
  ('00000000-0000-0000-0000-000000000002', '블루베리샵',    0.035),
  ('00000000-0000-0000-0000-000000000003', '체리스토어',    0.025),
  ('00000000-0000-0000-0000-000000000004', '딸기팜직송',    0.03),
  ('00000000-0000-0000-0000-000000000005', '레몬트리마켓',  0.04),
  ('00000000-0000-0000-0000-000000000006', '파인애플몰',    0.03),
  ('00000000-0000-0000-0000-000000000007', '수박직배송',    0.02),
  ('00000000-0000-0000-0000-000000000008', '포도나무상회',  0.035),
  ('00000000-0000-0000-0000-000000000009', '복숭아팜',      0.03),
  ('00000000-0000-0000-0000-000000000010', '키위익스프레스', 0.025);

-- ─────────────────────────────────────────────────────────────────────────────
-- 시드: 어제 날짜로 DELIVERED 주문 1,200건
-- (판매자별 약 120건, 총액 랜덤)
-- ─────────────────────────────────────────────────────────────────────────────

INSERT INTO orders (seller_id, status, total_amount, refund_amount, ordered_at, delivered_at)
SELECT
  s.id,
  'DELIVERED',
  -- 10,000 ~ 500,000 원 사이 랜덤 금액 (100원 단위)
  (floor(random() * 4910 + 100) * 100)::NUMERIC(12,2),
  -- 10% 확률로 환불 발생
  CASE WHEN random() < 0.1
    THEN (floor(random() * 50 + 1) * 1000)::NUMERIC(12,2)
    ELSE 0
  END,
  now() - interval '2 days',
  -- delivered_at = 어제
  (CURRENT_DATE - interval '1 day') + (random() * interval '23 hours')
FROM sellers s
CROSS JOIN generate_series(1, 120) AS gs(n);

-- ─────────────────────────────────────────────────────────────────────────────
-- 확인
-- ─────────────────────────────────────────────────────────────────────────────

SELECT
  s.name,
  COUNT(*)              AS order_count,
  SUM(o.total_amount)   AS total_sales,
  SUM(o.refund_amount)  AS total_refund
FROM orders o
JOIN sellers s ON s.id = o.seller_id
GROUP BY s.name
ORDER BY total_sales DESC;
