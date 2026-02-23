import { Entity, PrimaryKey, Property, ManyToOne, Enum } from '@mikro-orm/core';
import { v4 as uuidv4 } from 'uuid';
import { Point } from './point.entity';

export enum PointHistoryType {
  EARN = 'EARN',
  USE = 'USE',
  EXPIRE = 'EXPIRE',
  REFUND = 'REFUND',
}

@Entity({ tableName: 'point_histories' })
export class PointHistory {
  @PrimaryKey({ type: 'uuid' })
  id: string = uuidv4();

  @ManyToOne(() => Point, { fieldName: 'point_id' })
  point: Point;

  @Enum(() => PointHistoryType)
  type: PointHistoryType;

  @Property({ columnType: 'int' })
  amount: number;

  @Property({ length: 200, nullable: true })
  description?: string;

  @Property()
  createdAt: Date = new Date();
}
