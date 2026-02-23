import { Entity, PrimaryKey, Property, ManyToOne, Enum } from '@mikro-orm/core';
import { v4 as uuidv4 } from 'uuid';
import { Seller } from './seller.entity';

export enum OrderStatus {
  PENDING = 'PENDING',
  PAID = 'PAID',
  DELIVERED = 'DELIVERED',
  CANCELLED = 'CANCELLED',
  REFUNDED = 'REFUNDED',
}

@Entity({ tableName: 'orders' })
export class Order {
  @PrimaryKey({ type: 'uuid' })
  id: string = uuidv4();

  @ManyToOne(() => Seller, { fieldName: 'seller_id' })
  seller: Seller;

  @Enum(() => OrderStatus)
  status: OrderStatus;

  @Property({ columnType: 'numeric(12,2)' })
  totalAmount: number;

  @Property({ columnType: 'numeric(12,2)', default: 0 })
  refundAmount: number = 0;

  @Property()
  orderedAt: Date;

  @Property({ nullable: true })
  deliveredAt?: Date;

  @Property()
  createdAt: Date = new Date();
}
