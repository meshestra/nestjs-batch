import { Module } from '@nestjs/common';
import { ScheduleModule } from '@nestjs/schedule';
import { BatchModule } from '@nestjs-batch/core';
import { BatchScheduler } from './batch.scheduler';
import { DailySettlementJob } from './jobs/daily-settlement.job';
import { SettlementProcessor } from './jobs/settlement.processor';

@Module({
  imports: [
    ScheduleModule.forRoot(),

    // Rust sqlx 커넥션 풀을 초기화한다.
    // datasource.url → NativeDataSource.connect() → NATIVE_DATASOURCE_TOKEN으로 DI
    BatchModule.forRoot({
      datasource: {
        url: buildDatabaseUrl(),
        maxConnections: 10,
      },
    }),
  ],
  providers: [
    DailySettlementJob,
    SettlementProcessor,
    BatchScheduler,
  ],
})
export class AppModule {}

function buildDatabaseUrl(): string {
  const host = process.env.DB_HOST ?? 'localhost';
  const port = process.env.DB_PORT ?? '5432';
  const user = process.env.DB_USER ?? 'batch';
  const pass = process.env.DB_PASSWORD ?? 'batch';
  const name = process.env.DB_NAME ?? 'settlement';
  return `postgres://${user}:${pass}@${host}:${port}/${name}`;
}
