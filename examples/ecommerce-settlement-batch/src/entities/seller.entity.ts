import { Entity, PrimaryKey, Property, OneToMany, Collection } from '@mikro-orm/core';
import { v4 as uuidv4 } from 'uuid';
import { Order } from './order.entity';
import { SellerSettlement } from './seller-settlement.entity';

@Entity({ tableName: 'sellers' })
export class Seller {
  @PrimaryKey({ type: 'uuid' })
  id: string = uuidv4();

  @Property({ length: 100 })
  name: string;

  @Property({ length: 20, default: '0.03' })
  feeRate: string = '0.03'; // 수수료율 (기본 3%)

  @Property()
  createdAt: Date = new Date();

  @OneToMany(() => Order, (o) => o.seller)
  orders = new Collection<Order>(this);

  @OneToMany(() => SellerSettlement, (s) => s.seller)
  settlements = new Collection<SellerSettlement>(this);
}
