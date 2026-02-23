import { Entity, PrimaryKey, Property, ManyToOne, Enum } from '@mikro-orm/core';
import { v4 as uuidv4 } from 'uuid';
import { User } from './user.entity';

export enum CouponStatus {
  ACTIVE = 'ACTIVE',
  USED = 'USED',
  EXPIRED = 'EXPIRED',
}

@Entity({ tableName: 'coupons' })
export class Coupon {
  @PrimaryKey({ type: 'uuid' })
  id: string = uuidv4();

  @ManyToOne(() => User, { fieldName: 'user_id' })
  user: User;

  @Property({ length: 100 })
  name: string;

  @Property({ columnType: 'numeric(10,2)' })
  discountAmount: number;

  @Enum(() => CouponStatus)
  status: CouponStatus = CouponStatus.ACTIVE;

  @Property({ nullable: true })
  expiresAt?: Date;

  @Property({ nullable: true })
  usedAt?: Date;

  @Property()
  createdAt: Date = new Date();

  @Property({ onUpdate: () => new Date() })
  updatedAt: Date = new Date();
}
