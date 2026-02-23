import { Entity, PrimaryKey, Property, Enum, OneToMany, Collection } from '@mikro-orm/core';
import { v4 as uuidv4 } from 'uuid';
import { Point } from './point.entity';
import { Coupon } from './coupon.entity';

export enum UserStatus {
  ACTIVE = 'ACTIVE',
  DORMANT = 'DORMANT',
  WITHDRAWN = 'WITHDRAWN',
}

@Entity({ tableName: 'users' })
export class User {
  @PrimaryKey({ type: 'uuid' })
  id: string = uuidv4();

  @Property({ length: 100 })
  email: string;

  @Property({ length: 50 })
  name: string;

  @Enum(() => UserStatus)
  status: UserStatus = UserStatus.ACTIVE;

  @Property({ nullable: true })
  lastLoginAt?: Date;

  @Property({ nullable: true })
  dormantAt?: Date;

  @Property()
  createdAt: Date = new Date();

  @Property({ onUpdate: () => new Date() })
  updatedAt: Date = new Date();

  @OneToMany(() => Point, (p) => p.user)
  points = new Collection<Point>(this);

  @OneToMany(() => Coupon, (c) => c.user)
  coupons = new Collection<Coupon>(this);
}
