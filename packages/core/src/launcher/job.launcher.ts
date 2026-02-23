import { Inject, Injectable, Logger, Optional } from '@nestjs/common';
import {
  IJobLauncher,
  JobExecutionRecord,
  JobParameters,
  JobRepository,
  StepDefinition,
  StepExecutionRecord,
} from '../interfaces';
import { BatchRegistry } from '../registry/batch.registry';
import { JOB_REPOSITORY_TOKEN } from '../batch.constants';

// Rust 엔진 타입 (빌드 전에는 any로 처리)
// eslint-disable-next-line @typescript-eslint/no-explicit-any
type ChunkExecutorCtor = new (options: any) => any;

/**
 * Job 실행을 조율하는 핵심 서비스.
 *
 * 실행 흐름:
 * 1. BatchRegistry에서 JobDefinition 조회
 * 2. 중복 실행 방지 체크 (preventDuplicateRun)
 * 3. Step 순서대로 Rust ChunkExecutor 호출
 * 4. 각 Step 결과를 JobExecutionRecord로 집계
 * 5. JobRepository에 저장
 */
@Injectable()
export class JobLauncher implements IJobLauncher {
  private readonly logger = new Logger(JobLauncher.name);

  /**
   * jobExecutionId → ChunkExecutor 인스턴스 맵.
   * stop() 호출 시 해당 executor에 중단 신호를 전달하기 위해 보관한다.
   */
  private readonly runningExecutors = new Map<string, any>();

  constructor(
    private readonly registry: BatchRegistry,
    @Inject(JOB_REPOSITORY_TOKEN)
    private readonly jobRepository: JobRepository,
  ) {}

  async launch(
    jobName: string,
    jobParameters: JobParameters = {},
  ): Promise<JobExecutionRecord> {
    const jobDef = this.registry.getJob(jobName);
    if (!jobDef) {
      throw new Error(`Job not found: "${jobName}". Registered jobs: [${this.registry.getJobNames().join(', ')}]`);
    }

    // 중복 실행 방지
    if (jobDef.preventDuplicateRun !== false) {
      const running = await this.jobRepository.isRunning(jobName);
      if (running) {
        throw new Error(
          `Job "${jobName}" is already running. Set preventDuplicateRun=false to allow concurrent execution.`,
        );
      }
    }

    // 최상위 JobExecutionRecord 초기화
    const jobExecutionId = generateId();
    const jobRecord: JobExecutionRecord = {
      jobExecutionId,
      jobName,
      jobParameters,
      status: 'STARTED',
      readCount: 0,
      processCount: 0,
      writeCount: 0,
      skipCount: 0,
      commitCount: 0,
      startTime: Date.now(),
      endTime: null,
      durationMs: null,
      exitMessage: null,
      stepExecutions: [],
    };

    await this.jobRepository.save(jobRecord);
    this.logger.log(`Job "${jobName}" started. executionId=${jobExecutionId}`);

    try {
      // Step 순차 실행
      for (const stepDef of jobDef.steps) {
        const stepRecord = await this.runStep(
          jobExecutionId,
          jobName,
          stepDef,
        );

        jobRecord.stepExecutions.push(stepRecord);

        // 통계 누적
        jobRecord.readCount += stepRecord.readCount;
        jobRecord.processCount += stepRecord.processCount;
        jobRecord.writeCount += stepRecord.writeCount;
        jobRecord.skipCount +=
          stepRecord.readSkipCount +
          stepRecord.processSkipCount +
          stepRecord.writeSkipCount;
        jobRecord.commitCount += stepRecord.commitCount;

        // Step 실패 시 Job도 실패 처리하고 중단
        if (stepRecord.status === 'FAILED') {
          throw new Error(
            `Step "${stepDef.name}" failed. Aborting job "${jobName}".`,
          );
        }

        // STOPPED 처리
        if (stepRecord.status === 'STOPPED') {
          jobRecord.status = 'STOPPED';
          break;
        }
      }

      if (jobRecord.status !== 'STOPPED') {
        jobRecord.status = 'COMPLETED';
      }
    } catch (err) {
      jobRecord.status = 'FAILED';
      jobRecord.exitMessage = (err as Error).message;
      this.logger.error(
        `Job "${jobName}" failed: ${jobRecord.exitMessage}`,
      );
    } finally {
      jobRecord.endTime = Date.now();
      jobRecord.durationMs = jobRecord.endTime - jobRecord.startTime;
      this.runningExecutors.delete(jobExecutionId);
      await this.jobRepository.save(jobRecord);
    }

    this.logger.log(
      `Job "${jobName}" ${jobRecord.status}. ` +
        `read=${jobRecord.readCount} process=${jobRecord.processCount} ` +
        `write=${jobRecord.writeCount} duration=${jobRecord.durationMs}ms`,
    );

    return jobRecord;
  }

  async stop(jobExecutionId: string): Promise<void> {
    const executor = this.runningExecutors.get(jobExecutionId);
    if (!executor) {
      this.logger.warn(
        `stop() called but no running executor found for executionId=${jobExecutionId}`,
      );
      return;
    }
    executor.requestStop();
    this.logger.log(`Stop signal sent to executionId=${jobExecutionId}`);
  }

  // ─────────────────────────────────────────────────────────────────────────
  // 내부: Step 실행
  // ─────────────────────────────────────────────────────────────────────────

  private async runStep(
    jobExecutionId: string,
    jobName: string,
    stepDef: StepDefinition,
  ): Promise<StepExecutionRecord> {
    this.logger.log(
      `Step "${stepDef.name}" starting (chunkSize=${stepDef.chunkSize})`,
    );

    const ChunkExecutor = this.loadEngine();
    const executor = new ChunkExecutor({
      jobName,
      stepName: stepDef.name,
      chunkSize: stepDef.chunkSize,
      skipLimit: stepDef.skipLimit ?? 0,
      retryLimit: stepDef.retryLimit ?? 3,
      enableLogging: true,
    });

    // 실행 중 executor 등록 (stop 요청 대비)
    this.runningExecutors.set(jobExecutionId, executor);

    // Rust 엔진과의 콜백 규약:
    //   각 콜백은 (payload, reply: (json: string) => void) 시그니처를 가진다.
    //   비동기 작업 완료 후 reply(resultJson) 를 정확히 1회 호출해야 한다.
    //   오류 시: reply('__ERROR__:' + message)
    // NAPI-RS ErrorStrategy::CalleeHandled 는 JS 콜백 첫 번째 인수로 null(에러)을 자동 삽입한다.
    // 실제 JS 시그니처: (err: null, payload, reply) — err 는 항상 null 이므로 무시.
    const reader = (_err: null, offset: number, reply: (json: string) => void): void => {
      stepDef.reader.read(offset, stepDef.chunkSize)
        .then((items) => reply(JSON.stringify(items ?? null)))
        .catch((e: unknown) => reply('__ERROR__:' + String(e)));
    };

    const processor = (_err: null, itemsJson: string, reply: (json: string) => void): void => {
      const items: unknown[] = JSON.parse(itemsJson);
      stepDef.processor.process(items)
        .then((processed) => reply(JSON.stringify(processed)))
        .catch((e: unknown) => reply('__ERROR__:' + String(e)));
    };

    const writer = (_err: null, itemsJson: string, reply: (json: string) => void): void => {
      const items: unknown[] = JSON.parse(itemsJson);
      stepDef.writer.write(items)
        .then(() => reply(''))
        .catch((e: unknown) => reply('__ERROR__:' + String(e)));
    };

    const rawResult = await executor.execute(reader, processor, writer);

    return this.mapToStepRecord(stepDef.name, rawResult);
  }

  /**
   * Rust 엔진의 JobExecution 결과를 StepExecutionRecord로 변환한다.
   */
  private mapToStepRecord(stepName: string, raw: any): StepExecutionRecord {
    const stepJson: any =
      raw.stepExecutions && raw.stepExecutions.length > 0
        ? JSON.parse(raw.stepExecutions[0])
        : {};

    return {
      stepName,
      status: this.mapStatus(raw.status),
      readCount: raw.readCount ?? 0,
      processCount: raw.processCount ?? 0,
      writeCount: raw.writeCount ?? 0,
      readSkipCount: stepJson.read_skip_count ?? 0,
      processSkipCount: stepJson.process_skip_count ?? 0,
      writeSkipCount: stepJson.write_skip_count ?? 0,
      commitCount: raw.commitCount ?? 0,
      rollbackCount: stepJson.rollback_count ?? 0,
      startTime: raw.startTime ?? Date.now(),
      endTime: raw.endTime ?? null,
      durationMs: raw.durationMs ?? null,
    };
  }

  /** Rust status 문자열 → JobStatusType */
  private mapStatus(
    status: string,
  ): StepExecutionRecord['status'] {
    switch (status?.toUpperCase()) {
      case 'COMPLETED':
        return 'COMPLETED';
      case 'FAILED':
        return 'FAILED';
      case 'STOPPED':
        return 'STOPPED';
      default:
        return 'COMPLETED';
    }
  }

  /**
   * Rust 엔진 바인딩을 동적으로 로드한다.
   *
   * - 빌드된 `.node` 파일이 있으면 실제 엔진 사용
   * - 없으면 개발/테스트용 JS Fallback 사용
   */
  private loadEngine(): ChunkExecutorCtor {
    try {
      // eslint-disable-next-line @typescript-eslint/no-var-requires
      const engine = require('@nestjs-batch/engine');
      return engine.ChunkExecutor as ChunkExecutorCtor;
    } catch {
      this.logger.warn(
        'Rust engine not found. Falling back to JS-based ChunkExecutor for development.',
      );
      return JsFallbackExecutor as unknown as ChunkExecutorCtor;
    }
  }
}

// ─────────────────────────────────────────────────────────────────────────────
// 개발/테스트용 JS Fallback Executor
// (Rust 엔진 빌드 없이 동작 확인 가능)
// ─────────────────────────────────────────────────────────────────────────────

class JsFallbackExecutor {
  private stopped = false;

  constructor(private readonly options: any) {}

  requestStop(): void {
    this.stopped = true;
  }

  async execute(
    readerFn: (offset: number) => Promise<unknown[] | null>,
    processorFn: (items: unknown[]) => Promise<unknown[]>,
    writerFn: (items: unknown[]) => Promise<void>,
  ): Promise<any> {
    const { chunkSize, skipLimit = 0, retryLimit = 3 } = this.options;
    const startTime = Date.now();

    let offset = 0;
    let readCount = 0;
    let processCount = 0;
    let writeCount = 0;
    let skipCount = 0;
    let commitCount = 0;
    let rollbackCount = 0;
    let exitMessage: string | null = null;
    let status = 'COMPLETED';

    const stepInfo = {
      step_name: this.options.stepName,
      read_skip_count: 0,
      process_skip_count: 0,
      write_skip_count: 0,
      rollback_count: 0,
    };

    try {
      while (!this.stopped) {
        // Read
        let items: unknown[] | null;
        try {
          items = await readerFn(offset);
        } catch (err) {
          stepInfo.read_skip_count++;
          skipCount++;
          if (skipCount > skipLimit) {
            throw new Error(`Skip limit exceeded during read: ${(err as Error).message}`);
          }
          continue;
        }

        if (!items || items.length === 0) break;

        readCount += items.length;

        // Process
        let processed: unknown[];
        try {
          processed = await processorFn(items);
        } catch (err) {
          stepInfo.process_skip_count += items.length;
          skipCount += items.length;
          if (skipCount > skipLimit) {
            throw new Error(`Skip limit exceeded during process: ${(err as Error).message}`);
          }
          offset += chunkSize;
          continue;
        }

        processCount += processed.length;

        // Write (with retry)
        let written = false;
        for (let attempt = 0; attempt <= retryLimit; attempt++) {
          try {
            await writerFn(processed);
            writeCount += processed.length;
            commitCount++;
            written = true;
            break;
          } catch (err) {
            rollbackCount++;
            stepInfo.rollback_count++;
            if (attempt === retryLimit) {
              stepInfo.write_skip_count += processed.length;
              skipCount += processed.length;
              if (skipCount > skipLimit) {
                throw new Error(`Skip limit exceeded during write: ${(err as Error).message}`);
              }
            } else {
              await sleep(100 * Math.pow(2, attempt));
            }
          }
        }
        if (!written) {
          offset += chunkSize;
          continue;
        }

        offset += items.length;
        if (items.length < chunkSize) break;
      }

      if (this.stopped) {
        status = 'STOPPED';
        exitMessage = 'Stopped by external request';
      }
    } catch (err) {
      status = 'FAILED';
      exitMessage = (err as Error).message;
    }

    const endTime = Date.now();

    return {
      jobExecutionId: generateId(),
      jobName: this.options.jobName,
      status,
      readCount,
      processCount,
      writeCount,
      skipCount,
      commitCount,
      startTime,
      endTime,
      durationMs: endTime - startTime,
      exitMessage,
      stepExecutions: [JSON.stringify({
        ...stepInfo,
        rollback_count: rollbackCount,
      })],
    };
  }
}

function generateId(): string {
  return `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 9)}`;
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
