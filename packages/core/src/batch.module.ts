import {
  DynamicModule,

  FactoryProvider,

  Module,

  ModuleMetadata,

  Provider,

  Type,
} from '@nestjs/common';
import { DiscoveryModule } from '@nestjs/core';
import {
  BATCH_MODULE_OPTIONS_TOKEN,
  JOB_REPOSITORY_TOKEN,
} from './batch.constants';
import { JobRepository } from './interfaces';
import { JobLauncher } from './launcher/job.launcher';
import { BatchRegistry } from './registry/batch.registry';
import { InMemoryJobRepository } from './repositories/in-memory-job.repository';

// ─────────────────────────────────────────────────────────────────────────────
// 옵션 타입
// ─────────────────────────────────────────────────────────────────────────────

/**
 * `BatchModule.forRoot()` 동기 옵션
 */
export interface BatchModuleOptions {
  /**
   * 커스텀 JobRepository 구현체.
   * 생략 시 `InMemoryJobRepository`가 사용된다.
   *
   * @example
   * ```typescript
   * BatchModule.forRoot({ jobRepository: PrismaJobRepository })
   * ```
   */
  jobRepository?: Type<JobRepository>;
}

/**
 * `BatchModule.forRootAsync()` 비동기 옵션
 */
export interface BatchModuleAsyncOptions extends Pick<ModuleMetadata, 'imports'> {
  /** 옵션 팩토리 함수를 제공할 클래스 */
  useFactory?: (...args: any[]) => Promise<BatchModuleOptions> | BatchModuleOptions;
  inject?: any[];
}

// ─────────────────────────────────────────────────────────────────────────────
// BatchModule
// ─────────────────────────────────────────────────────────────────────────────

/**
 * nestjs-batch 프레임워크의 루트 NestJS 모듈.
 *
 * 앱 루트 모듈(AppModule)에서 `BatchModule.forRoot()`로 등록한다.
 *
 * @example 기본 사용 (InMemoryJobRepository)
 * ```typescript
 * @Module({
 *   imports: [BatchModule.forRoot()],
 * })
 * export class AppModule {}
 * ```
 *
 * @example 커스텀 Repository
 * ```typescript
 * @Module({
 *   imports: [
 *     BatchModule.forRoot({ jobRepository: PrismaJobRepository }),
 *   ],
 * })
 * export class AppModule {}
 * ```
 *
 * @example 비동기 등록 (ConfigService 활용)
 * ```typescript
 * BatchModule.forRootAsync({
 *   imports: [ConfigModule],
 *   useFactory: (config: ConfigService) => ({
 *     jobRepository: config.get('USE_DB') ? PrismaJobRepository : undefined,
 *   }),
 *   inject: [ConfigService],
 * })
 * ```
 */
@Module({})
export class BatchModule {
  /**
   * 동기 방식으로 `BatchModule` 을 등록한다.
   */
  static forRoot(options: BatchModuleOptions = {}): DynamicModule {
    const repoProvider = BatchModule.createRepositoryProvider(options);

    return {
      module: BatchModule,
      imports: [DiscoveryModule],
      providers: [
        repoProvider,
        BatchRegistry,
        JobLauncher,
      ],
      exports: [
        JOB_REPOSITORY_TOKEN,
        BatchRegistry,
        JobLauncher,
      ],
      global: true,
    };
  }

  /**
   * 비동기 방식으로 `BatchModule` 을 등록한다.
   * ConfigService 등 DI를 통해 옵션을 주입받아야 할 때 사용한다.
   */
  static forRootAsync(asyncOptions: BatchModuleAsyncOptions): DynamicModule {
    const asyncProvider: Provider = {
      provide: BATCH_MODULE_OPTIONS_TOKEN,
      useFactory: asyncOptions.useFactory ?? (() => ({})),
      inject: asyncOptions.inject ?? [],
    };

    const repoProvider: FactoryProvider = {
      provide: JOB_REPOSITORY_TOKEN,
      useFactory: (opts: BatchModuleOptions): JobRepository => {
        const RepoClass = opts.jobRepository ?? InMemoryJobRepository;
        return new RepoClass();
      },
      inject: [BATCH_MODULE_OPTIONS_TOKEN],
    };

    return {
      module: BatchModule,
      imports: [DiscoveryModule, ...(asyncOptions.imports ?? [])],
      providers: [
        asyncProvider,
        repoProvider,
        BatchRegistry,
        JobLauncher,
      ],
      exports: [
        JOB_REPOSITORY_TOKEN,
        BatchRegistry,
        JobLauncher,
      ],
      global: true,
    };
  }

  // ─────────────────────────────────────────────────────────────────────────
  // 내부 헬퍼
  // ─────────────────────────────────────────────────────────────────────────

  private static createRepositoryProvider(
    options: BatchModuleOptions,
  ): Provider {
    if (options.jobRepository) {
      // 사용자 제공 Repository 클래스를 DI 토큰으로 등록
      return {
        provide: JOB_REPOSITORY_TOKEN,
        useClass: options.jobRepository,
      };
    }

    // 기본값: InMemoryJobRepository
    return {
      provide: JOB_REPOSITORY_TOKEN,
      useClass: InMemoryJobRepository,
    };
  }
}
