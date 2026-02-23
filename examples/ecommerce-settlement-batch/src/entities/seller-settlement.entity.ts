import { Entity, PrimaryKey, Property, ManyToOne, Enum } from '@mikro-orm/core';
import { v4 as uuidv4 } from 'uuid';
import { Seller } from './seller.entity';

export enum SettlementStatus {
  COMPLETED = 'COMPLETED',
  FAILED = 'FAILED',
}

@Entity({ tableName: 'seller_settlements' })
export class SellerSettlement {
  @PrimaryKey({ type: 'uuid' })
  id: string = uuidv4();

  @ManyToOne(() => Seller, { fieldName: 'seller_id' })
  seller: Seller;

  @Property({ columnType: 'date' })
  settlementDate: string; // YYYY-MM-DD

  @Property({ columnType: 'numeric(12,2)' })
  totalSalesAmount: number; // 총 매출액

  @Property({ columnType: 'numeric(12,2)' })
  feeAmount: number; // 수수료

  @Property({ columnType: 'numeric(12,2)' })
  settlementAmount: number; // 정산액 (매출 - 수수료 - 환불)

  @Property({ columnType: 'numeric(12,2)', default: 0 })
  refundAmount: number = 0; // 환불액

  @Enum(() => SettlementStatus)
  status: SettlementStatus;

  @Property({ nullable: true, length: 500 })
  failReason?: string;

  @Property()
  createdAt: Date = new Date();

  @Property({ onUpdate: () => new Date() })
  updatedAt: Date = new Date();
}
