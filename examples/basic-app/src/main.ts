import 'reflect-metadata';
import { NestFactory } from '@nestjs/core';
import { AppModule } from './app.module';
import { JobLauncher } from '@nestjs-batch/core';

/**
 * 샘플 앱 진입점.
 *
 * NestJS 앱을 초기화하고 "user-point-settlement" Job을 실행한 뒤
 * 결과를 콘솔에 출력하고 종료한다.
 *
 * 실제 앱에서는 HTTP 엔드포인트나 스케줄러(@nestjs/schedule)를 통해
 * JobLauncher.launch()를 호출하는 것이 일반적이다.
 */
async function bootstrap(): Promise<void> {
  // NestJS 앱 컨텍스트 생성 (HTTP 서버 없이 배치 전용으로 실행)
  const app = await NestFactory.createApplicationContext(AppModule, {
    logger: ['log', 'warn', 'error'],
  });

  const launcher = app.get(JobLauncher);

  console.log('\n==============================');
  console.log(' nestjs-batch 샘플 실행 시작');
  console.log('==============================\n');

  try {
    const result = await launcher.launch('user-point-settlement', {
      runDate: new Date().toISOString(),
      triggeredBy: 'manual',
    });

    console.log('\n==============================');
    console.log(' Job 실행 결과');
    console.log('==============================');
    console.log(`  상태       : ${result.status}`);
    console.log(`  실행 ID    : ${result.jobExecutionId}`);
    console.log(`  읽기       : ${result.readCount}`);
    console.log(`  처리       : ${result.processCount}`);
    console.log(`  쓰기       : ${result.writeCount}`);
    console.log(`  스킵       : ${result.skipCount}`);
    console.log(`  커밋       : ${result.commitCount}`);
    console.log(`  소요 시간  : ${result.durationMs}ms`);
    if (result.exitMessage) {
      console.log(`  종료 메시지: ${result.exitMessage}`);
    }
    console.log('==============================\n');
  } catch (err) {
    console.error('Job 실행 중 오류 발생:', (err as Error).message);
  } finally {
    await app.close();
  }
}

bootstrap().catch(console.error);
