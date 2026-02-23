import { Injectable, Logger } from '@nestjs/common';
import { Cron, CronExpression } from '@nestjs/schedule';
import { JobLauncher } from '@nestjs-batch/core';

/**
 * 배치 스케줄러.
 *
 * @nestjs/schedule의 @Cron 데코레이터로 주기적으로 Job을 트리거한다.
 * JobLauncher는 BatchModule이 전역 모듈로 제공하므로 별도 import 없이 주입받는다.
 */
@Injectable()
export class BatchScheduler {
  private readonly logger = new Logger(BatchScheduler.name);

  constructor(private readonly jobLauncher: JobLauncher) {}

  /**
   * 매일 새벽 2시: 전날 정산 실행
   * 예) 2024-01-16 02:00 → targetDate=2024-01-15
   */
  @Cron(CronExpression.EVERY_DAY_AT_2AM)
  async runDailySettlement(): Promise<void> {
    const yesterday = new Date();
    yesterday.setDate(yesterday.getDate() - 1);
    const targetDate = yesterday.toISOString().slice(0, 10);

    this.logger.log(`[Cron] daily-settlement 시작 targetDate=${targetDate}`);
    try {
      const result = await this.jobLauncher.launch('daily-settlement', {
        targetDate,
      });
      this.logger.log(
        `[Cron] daily-settlement 완료 status=${result.status} ` +
          `read=${result.readCount} write=${result.writeCount} ` +
          `duration=${result.durationMs}ms`,
      );
    } catch (err) {
      this.logger.error(`[Cron] daily-settlement 실패: ${(err as Error).message}`);
    }
  }
}
