/**
 * NestJS DI 토큰 상수
 *
 * JobRepository 구현체를 주입할 때 사용하는 토큰.
 * 사용자가 커스텀 `Repository` 를 제공하거나 기본 `InMemoryJobRepository` 를 사용할 때
 * 동일한 토큰으로 주입된다.
 */
export const JOB_REPOSITORY_TOKEN = 'NESTJS_BATCH_JOB_REPOSITORY';

/** BatchModule 옵션 주입 토큰 */
export const BATCH_MODULE_OPTIONS_TOKEN = 'NESTJS_BATCH_MODULE_OPTIONS';
