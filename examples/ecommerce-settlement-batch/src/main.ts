import { NestFactory } from '@nestjs/core';
import { Logger } from '@nestjs/common';
import { JobLauncher } from '@nestjs-batch/core';
import { AppModule } from './app.module';

const logger = new Logger('Bootstrap');

async function bootstrap() {
  const app = await NestFactory.createApplicationContext(AppModule);

  const jobLauncher = app.get(JobLauncher);

  // 실행할 targetDate: CLI 인수 또는 어제 날짜
  const targetDate =
    process.argv[2] ?? new Date(Date.now() - 86_400_000).toISOString().slice(0, 10);

  logger.log(`daily-settlement 실행 시작 — targetDate=${targetDate}`);

  const result = await jobLauncher.launch('daily-settlement', { targetDate });

  logger.log('─'.repeat(60));
  logger.log(`status       : ${result.status}`);
  logger.log(`readCount    : ${result.readCount}`);
  logger.log(`processCount : ${result.processCount}`);
  logger.log(`writeCount   : ${result.writeCount}`);
  logger.log(`commitCount  : ${result.commitCount}`);
  logger.log(`duration     : ${result.durationMs}ms`);
  logger.log('─'.repeat(60));

  await app.close();
}

bootstrap().catch((err) => {
  logger.error('배치 실행 실패', err);
  process.exit(1);
});
