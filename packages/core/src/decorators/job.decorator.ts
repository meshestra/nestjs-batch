import { Injectable } from '@nestjs/common';
import { JobDecoratorOptions } from '../interfaces';
import { JOB_METADATA_KEY } from './constants';

/**
 * 클래스를 NestJS-Batch Job으로 등록하는 클래스 데코레이터.
 *
 * `@Job()`이 붙은 클래스는 NestJS DI 컨테이너에 `@Injectable()`로 등록되고,
 * `BatchRegistry`가 앱 시작 시 자동으로 스캔하여 Job 목록에 추가한다.
 *
 * @param options Job 설정 옵션. 생략 시 클래스 이름을 Job 이름으로 사용.
 *
 * @example
 * ```typescript
 * @Job({ name: 'user-migration', preventDuplicateRun: true })
 * export class UserMigrationJob {
 *   @Step({ chunkSize: 500 })
 *   migrateUsers(): StepDefinition { ... }
 * }
 * ```
 */
export function Job(options: JobDecoratorOptions = {}): ClassDecorator {
  return (target: Function) => {
    // NestJS Injectable로 자동 등록
    Injectable()(target);

    // Job 메타데이터 부착
    Reflect.defineMetadata(JOB_METADATA_KEY, options, target);
  };
}
