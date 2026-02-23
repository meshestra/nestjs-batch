import { Entity, PrimaryKey, Property, ManyToOne, OneToMany, Collection } from '@mikro-orm/core';
import { v4 as uuidv4 } from 'uuid';
import { User } from './user.entity';
import { PointHistory } from './point-history.entity';

@Entity({ tableName: 'points' })
export class Point {
  @PrimaryKey({ type: 'uuid' })
  id: string = uuidv4();

  @ManyToOne(() => User, { fieldName: 'user_id' })
  user: User;

  @Property({ columnType: 'int', default: 0 })
  balance: number = 0;

  @Property({ nullable: true })
  expiresAt?: Date;

  @Property()
  createdAt: Date = new Date();

  @Property({ onUpdate: () => new Date() })
  updatedAt: Date = new Date();

  @OneToMany(() => PointHistory, (h) => h.point)
  histories = new Collection<PointHistory>(this);
}
