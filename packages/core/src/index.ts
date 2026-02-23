/**
 * @nestjs-batch/core - Public API
 *
 * 사용자가 import하는 모든 심볼은 이 파일을 통해 노출된다.
 *
 * @example
 * ```typescript
 * import {
 *   BatchModule,
 *   Job,
 *   Step,
 *   JobLauncher,
 *   ItemReader,
 *   ItemProcessor,
 *   ItemWriter,
 * } from '@nestjs-batch/core';
 * ```
 */

// 모듈
export { BatchModule } from './batch.module';
export type { BatchModuleOptions, BatchModuleAsyncOptions } from './batch.module';

// 데코레이터
export { Job, Step } from './decorators';

// 인터페이스 & 타입
export type {
  ItemReader,
  ItemProcessor,
  ItemWriter,
  JobStatusType,
  JobParameters,
  StepExecutionRecord,
  JobExecutionRecord,
  JobRepository,
  StepDefinition,
  JobDefinition,
  IJobLauncher,
  JobDecoratorOptions,
  StepDecoratorOptions,
} from './interfaces';

// 서비스
export { JobLauncher } from './launcher';
export { BatchRegistry } from './registry';

// Repository 구현체
export { InMemoryJobRepository } from './repositories';

// 상수
export { JOB_REPOSITORY_TOKEN } from './batch.constants';
