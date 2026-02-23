import { Injectable } from '@nestjs/common';
import {
  JobExecutionRecord,
  JobRepository,
  JobStatusType,
} from '../interfaces';

/**
 * 인메모리 기반 JobRepository 기본 구현체.
 *
 * 별도의 DB 설정 없이 즉시 사용할 수 있는 기본 구현이다.
 * 프로세스가 재시작되면 모든 이력이 초기화된다.
 *
 * 운영 환경에서는 `JobRepository` 인터페이스를 직접 구현한
 * DB 기반 Repository(예: Prisma, TypeORM)로 교체하는 것을 권장한다.
 */
@Injectable()
export class InMemoryJobRepository implements JobRepository {
  /** jobExecutionId → JobExecutionRecord 맵 */
  private readonly store = new Map<string, JobExecutionRecord>();

  async save(record: JobExecutionRecord): Promise<void> {
    this.store.set(record.jobExecutionId, { ...record });
  }

  async findById(jobExecutionId: string): Promise<JobExecutionRecord | null> {
    return this.store.get(jobExecutionId) ?? null;
  }

  async findAllByJobName(
    jobName: string,
    limit = 20,
  ): Promise<JobExecutionRecord[]> {
    const results: JobExecutionRecord[] = [];
    for (const record of this.store.values()) {
      if (record.jobName === jobName) {
        results.push(record);
      }
    }
    // 최신 순 정렬
    results.sort((a, b) => b.startTime - a.startTime);
    return results.slice(0, limit);
  }

  async findLastExecution(jobName: string): Promise<JobExecutionRecord | null> {
    const records = await this.findAllByJobName(jobName, 1);
    return records[0] ?? null;
  }

  async findByStatus(status: JobStatusType): Promise<JobExecutionRecord[]> {
    const results: JobExecutionRecord[] = [];
    for (const record of this.store.values()) {
      if (record.status === status) {
        results.push(record);
      }
    }
    return results;
  }

  async isRunning(jobName: string): Promise<boolean> {
    for (const record of this.store.values()) {
      if (record.jobName === jobName && record.status === 'STARTED') {
        return true;
      }
    }
    return false;
  }

  async updateStatus(
    jobExecutionId: string,
    status: JobStatusType,
    exitMessage?: string,
  ): Promise<void> {
    const record = this.store.get(jobExecutionId);
    if (!record) return;

    record.status = status;
    if (exitMessage !== undefined) {
      record.exitMessage = exitMessage;
    }
    if (status === 'COMPLETED' || status === 'FAILED' || status === 'STOPPED') {
      record.endTime = Date.now();
      record.durationMs =
        record.endTime - record.startTime;
    }
    this.store.set(jobExecutionId, record);
  }

  async purgeOlderThan(olderThanMs: number): Promise<number> {
    let count = 0;
    for (const [id, record] of this.store.entries()) {
      if (record.startTime < olderThanMs) {
        this.store.delete(id);
        count++;
      }
    }
    return count;
  }

  /** 전체 저장된 레코드 수 (테스트/디버그용) */
  size(): number {
    return this.store.size;
  }

  /** 전체 초기화 (테스트용) */
  clear(): void {
    this.store.clear();
  }
}
