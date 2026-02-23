import { Injectable } from '@nestjs/common';
import { Job, Step } from '@nestjs-batch/core';
import type { JobParameters, NativeStepDefinition } from '@nestjs-batch/core';
import { SettlementProcessor, SettlementResult } from './settlement.processor';

/**
 * 일별 판매자 정산 Job.
 *
 * ## 실행 흐름
 *
 * ```
 * Rust: DbReader (SELECT 집계)
 *   │  ← 직렬화 없음 (Rust 내부 Value)
 *   ▼
 * JS:   SettlementProcessor (수수료·정산액 계산)
 *   │  ← 직렬화 2회 (NAPI 경계)
 *   ▼
 * Rust: DbWriter (INSERT … ON CONFLICT DO UPDATE)
 * ```
 *
 * ## 사용 예
 *
 * ```typescript
 * jobLauncher.launch('daily-settlement', { targetDate: '2024-01-15' });
 * ```
 */
@Injectable()
@Job({ name: 'daily-settlement', preventDuplicateRun: true })
export class DailySettlementJob {
  constructor(private readonly processor: SettlementProcessor) {}

  @Step({ order: 1, chunkSize: 500 })
  settleStep(params: JobParameters): NativeStepDefinition {
    const targetDate = (params.targetDate as string) ?? new Date().toISOString().slice(0, 10);

    return {
      name: 'settle-step',
      chunkSize: 500,

      // ── Reader: Rust sqlx가 직접 SELECT ───────────────────────────────────
      // LIMIT / OFFSET 은 Rust 엔진이 자동으로 붙인다.
      reader: {
        // NUMERIC 컬럼은 sqlx가 기본 float으로 decode하지 못하므로
        // CAST(... AS FLOAT8)로 명시적 변환하여 Rust가 f64로 읽도록 한다.
        // UUID 컬럼도 ::text로 캐스트하여 String으로 읽는다.
        query: `
          SELECT
            o.seller_id::text                    AS seller_id,
            s.fee_rate::float8                   AS fee_rate,
            SUM(o.total_amount)::float8          AS sales,
            SUM(o.refund_amount)::float8         AS refund
          FROM orders o
          JOIN sellers s ON s.id = o.seller_id
          WHERE o.status    = 'DELIVERED'
            AND o.delivered_at::date = '${targetDate}'
          GROUP BY o.seller_id, s.fee_rate
        `,
      },

      // ── Processor: 수수료 계산은 JS에서 ──────────────────────────────────
      processor: this.processor,

      // ── Writer: Rust sqlx가 직접 INSERT ──────────────────────────────────
      writer: {
        query: (item: unknown): { sql: string; params: unknown[] } => {
          const row = item as SettlementResult;
          return {
            sql: `
              INSERT INTO seller_settlements
                (id, seller_id, settlement_date,
                 total_sales_amount, fee_amount, refund_amount,
                 settlement_amount, status, created_at, updated_at)
              VALUES
                (gen_random_uuid(), $1::uuid, $2::date,
                 $3::numeric, $4::numeric, $5::numeric, $6::numeric,
                 'COMPLETED', now(), now())
              ON CONFLICT (seller_id, settlement_date)
              DO UPDATE SET
                total_sales_amount = EXCLUDED.total_sales_amount,
                fee_amount         = EXCLUDED.fee_amount,
                refund_amount      = EXCLUDED.refund_amount,
                settlement_amount  = EXCLUDED.settlement_amount,
                status             = 'COMPLETED',
                updated_at         = now()
            `,
            params: [
              row.sellerId,
              targetDate,
              row.totalSalesAmount,
              row.feeAmount,
              row.refundAmount,
              row.settlementAmount,
            ],
          };
        },
      },

      skipLimit: 0,
      retryLimit: 3,
    };
  }
}
