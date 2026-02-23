import { Injectable } from '@nestjs/common';
import { ItemProcessor } from '@nestjs-batch/core';

/** DbReader가 읽어온 원시 집계 row */
export interface SettlementRaw {
  seller_id: string;
  fee_rate: string;   // numeric → string (sqlx 기본 직렬화)
  sales: string;      // SUM(total_amount)
  refund: string;     // SUM(refund_amount)
}

/** Processor가 계산을 마친 결과 */
export interface SettlementResult {
  sellerId: string;
  totalSalesAmount: number;
  feeAmount: number;
  refundAmount: number;
  settlementAmount: number;
}

/**
 * 수수료 계산 Processor.
 *
 * Reader(Rust sqlx) → **SettlementProcessor(JS)** → Writer(Rust sqlx)
 *
 * 플랫폼별 수수료율은 sellers.fee_rate 컬럼에서 읽어온다.
 * 정산액 = 매출 - 수수료 - 환불
 */
@Injectable()
export class SettlementProcessor
  implements ItemProcessor<SettlementRaw, SettlementResult>
{
  async process(items: SettlementRaw[]): Promise<SettlementResult[]> {
    return items.map((row) => {
      const sales = parseFloat(row.sales ?? '0');
      const refund = parseFloat(row.refund ?? '0');
      const feeRate = parseFloat(row.fee_rate ?? '0.03');

      const feeAmount = Math.round(sales * feeRate * 100) / 100;
      const settlementAmount =
        Math.round((sales - feeAmount - refund) * 100) / 100;

      return {
        sellerId: row.seller_id,
        totalSalesAmount: sales,
        feeAmount,
        refundAmount: refund,
        settlementAmount,
      };
    });
  }
}
