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
  NATIVE_DATASOURCE_TOKEN,
} from './batch.constants';
import { JobRepository } from './interfaces';
import { JobLauncher } from './launcher/job.launcher';
import { BatchRegistry } from './registry/batch.registry';
import { InMemoryJobRepository } from './repositories/in-memory-job.repository';

// ─────────────────────────────────────────────────────────────────────────────
// 옵션 타입
// ─────────────────────────────────────────────────────────────────────────────

/**
 * Rust sqlx 커넥션 풀 옵션.
 * `BatchModule.forRoot({ datasource: { url } })` 로 전달한다.
 */
export interface DataSourceOptions {
  /** DB 접속 URL. `postgres://` 또는 `mysql://` 프로토콜을 지원한다. */
  url: string;
  /** 최대 커넥션 수 (기본값: 10) */
  maxConnections?: number;
}

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

  /**
   * Rust sqlx 커넥션 풀 설정.
   * 제공 시 `NativeStepDefinition`에서 Rust가 DB I/O를 직접 처리한다.
   *
   * @example
   * ```typescript
   * BatchModule.forRoot({
   *   datasource: { url: process.env.DATABASE_URL, maxConnections: 10 },
   * })
   * ```
   */
  datasource?: DataSourceOptions;
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
    const datasourceProvider = BatchModule.createDatasourceProvider(options);

    return {
      module: BatchModule,
      imports: [DiscoveryModule],
      providers: [
        repoProvider,
        datasourceProvider,
        BatchRegistry,
        JobLauncher,
      ],
      exports: [
        JOB_REPOSITORY_TOKEN,
        NATIVE_DATASOURCE_TOKEN,
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

    const datasourceProvider: FactoryProvider = {
      provide: NATIVE_DATASOURCE_TOKEN,
      useFactory: (opts: BatchModuleOptions) =>
        BatchModule.connectDatasource(opts),
      inject: [BATCH_MODULE_OPTIONS_TOKEN],
    };

    return {
      module: BatchModule,
      imports: [DiscoveryModule, ...(asyncOptions.imports ?? [])],
      providers: [
        asyncProvider,
        repoProvider,
        datasourceProvider,
        BatchRegistry,
        JobLauncher,
      ],
      exports: [
        JOB_REPOSITORY_TOKEN,
        NATIVE_DATASOURCE_TOKEN,
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
      return {
        provide: JOB_REPOSITORY_TOKEN,
        useClass: options.jobRepository,
      };
    }
    return {
      provide: JOB_REPOSITORY_TOKEN,
      useClass: InMemoryJobRepository,
    };
  }

  private static createDatasourceProvider(
    options: BatchModuleOptions,
  ): FactoryProvider {
    return {
      provide: NATIVE_DATASOURCE_TOKEN,
      useFactory: () => BatchModule.connectDatasource(options),
    };
  }

  /**
   * datasource 옵션이 있으면 `NativeDataSource.connect()`를 호출하고,
   * 없으면 null을 반환한다.
   *
   * JobLauncher는 null 여부를 체크하여 Native 실행 경로를 선택한다.
   */
  private static async connectDatasource(
    options: BatchModuleOptions,
  ): Promise<unknown> {
    if (!options.datasource) return null;

    // 빌드된 Rust 엔진이 없는 환경(개발/CI)에서도 graceful하게 처리
    try {
      // eslint-disable-next-line @typescript-eslint/no-var-requires
      const engine = require('@nestjs-batch/engine');
      return await engine.NativeDataSource.connect(
        options.datasource.url,
        options.datasource.maxConnections,
      );
    } catch {
      // 엔진 바이너리가 없으면 null 반환 (JS 콜백 경로로 폴백)
      return null;
    }
  }
}
