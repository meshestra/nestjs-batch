import { Injectable, Logger, OnModuleInit } from '@nestjs/common';
import { DiscoveryService, ModuleRef } from '@nestjs/core';
import { InstanceWrapper } from '@nestjs/core/injector/instance-wrapper';
import {
  JOB_METADATA_KEY,
  STEP_METADATA_KEY,
  STEP_METHODS_METADATA_KEY,
} from '../decorators/constants';
import {
  JobDecoratorOptions,
  JobDefinition,
  StepDecoratorOptions,
  StepDefinition,
} from '../interfaces';
import { toKebabCase } from '../utils/string.utils';

/**
 * 앱 전체에 등록된 Job 목록을 관리하는 레지스트리.
 *
 * `OnModuleInit` 훅에서 `DiscoveryService`로 모든 Provider를 스캔하고,
 * `@Job()` 메타데이터가 붙은 클래스의 `@Step()` 메서드를 수집하여
 * `JobDefinition` 맵으로 구성한다.
 */
@Injectable()
export class BatchRegistry implements OnModuleInit {
  private readonly logger = new Logger(BatchRegistry.name);
  private readonly jobs = new Map<string, JobDefinition>();

  constructor(
    private readonly discovery: DiscoveryService,
    private readonly moduleRef: ModuleRef,
  ) { }

  onModuleInit(): void {
    this.scanJobs();
  }

  /**
   * 등록된 모든 Job을 스캔하여 JobDefinition 맵을 구성한다.
   * NestJS 앱 초기화 완료 후 자동으로 호출된다.
   */
  private scanJobs(): void {
    const providers: InstanceWrapper[] = this.discovery.getProviders();

    for (const wrapper of providers) {
      const { instance } = wrapper;
      if (!instance || !instance.constructor) continue;

      const jobMeta: JobDecoratorOptions | undefined = Reflect.getMetadata(
        JOB_METADATA_KEY,
        instance.constructor,
      );
      if (!jobMeta) continue;

      // Job 이름: 옵션 > 클래스 이름 (PascalCase → kebab-case)
      const jobName =
        jobMeta.name ?? toKebabCase(instance.constructor.name);

      const steps = this.collectSteps(instance);

      if (steps.length === 0) {
        this.logger.warn(
          `Job "${jobName}" has no @Step() methods. Skipping registration.`,
        );
        continue;
      }

      const jobDefinition: JobDefinition = {
        name: jobName,
        steps,
        preventDuplicateRun: jobMeta.preventDuplicateRun ?? true,
        description: jobMeta.description,
      };

      this.jobs.set(jobName, jobDefinition);
      this.logger.log(
        `Registered Job: "${jobName}" with ${steps.length} step(s) → [${steps.map((s) => s.name).join(', ')}]`,
      );
    }




  }

  /**
   * Job 클래스 인스턴스에서 @Step() 메서드를 수집하여 StepDefinition 배열로 반환한다.
   */
  private collectSteps(instance: any): StepDefinition[] {
    const stepMethods: (string | symbol)[] =
      Reflect.getOwnMetadata(
        STEP_METHODS_METADATA_KEY,
        instance.constructor.prototype,
      ) ?? [];

    const stepsWithOrder: Array<{ step: StepDefinition; order: number }> = [];

    for (const methodKey of stepMethods) {
      const stepOptions: StepDecoratorOptions =
        Reflect.getMetadata(
          STEP_METADATA_KEY,
          instance.constructor.prototype,
          methodKey,
        ) ?? {};

      // @Step() 붙은 메서드를 호출하여 StepDefinition을 가져옴
      let stepDef: StepDefinition;
      try {
        stepDef = instance[methodKey]();
      } catch (err) {
        this.logger.error(
          `Failed to collect step from method "${String(methodKey)}": ${(err as Error).message}`,
        );
        continue;
      }

      // 메서드 이름에서 Step 이름 보완
      if (!stepDef.name) {
        stepDef.name =
          stepOptions.name ?? toKebabCase(String(methodKey));
      }

      // 데코레이터 옵션으로 기본값 보완 (StepDefinition이 직접 지정한 값 우선)
      stepDef.chunkSize = stepDef.chunkSize ?? stepOptions.chunkSize ?? 100;
      stepDef.skipLimit = stepDef.skipLimit ?? stepOptions.skipLimit ?? 0;
      stepDef.retryLimit = stepDef.retryLimit ?? stepOptions.retryLimit ?? 3;

      stepsWithOrder.push({ step: stepDef, order: stepOptions.order ?? 0 });
    }

    // order 오름차순 정렬 → Step 실행 순서 결정
    stepsWithOrder.sort((a, b) => a.order - b.order);
    return stepsWithOrder.map((s) => s.step);
  }

  /** Job 이름으로 JobDefinition을 조회한다. */
  getJob(jobName: string): JobDefinition | undefined {
    return this.jobs.get(jobName);
  }

  /** 등록된 모든 Job 이름 목록을 반환한다. */
  getJobNames(): string[] {
    return Array.from(this.jobs.keys());
  }

  /** 등록된 모든 JobDefinition을 반환한다. */
  getAllJobs(): JobDefinition[] {
    return Array.from(this.jobs.values());
  }

}
