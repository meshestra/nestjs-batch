import { Injectable } from '@nestjs/common';
import {
  Job,
  Step,
  ItemReader,
  ItemProcessor,
  ItemWriter,
  StepDefinition,
} from '@nestjs-batch/core';

// ─────────────────────────────────────────────────────────────────────────────
// 도메인 타입
// ─────────────────────────────────────────────────────────────────────────────

interface User {
  id: number;
  name: string;
  rawPoints: number; // 원시 포인트 (정산 전)
}

interface SettledUser {
  id: number;
  name: string;
  finalPoints: number; // 정산된 포인트 (세금 10% 공제)
  settledAt: Date;
}

// ─────────────────────────────────────────────────────────────────────────────
// Reader: 더미 유저 100명을 chunk 단위로 읽기
// ─────────────────────────────────────────────────────────────────────────────

@Injectable()
export class UserPointReader implements ItemReader<User> {
  private static readonly TOTAL_USERS = 100;

  async read(offset: number, chunkSize: number): Promise<User[] | null> {
    if (offset >= UserPointReader.TOTAL_USERS) return null;

    const users: User[] = [];
    const end = Math.min(offset + chunkSize, UserPointReader.TOTAL_USERS);

    for (let i = offset; i < end; i++) {
      users.push({
        id: i + 1,
        name: `User-${i + 1}`,
        rawPoints: Math.floor(Math.random() * 10_000) + 1_000,
      });
    }

    return users;
  }
}

// ─────────────────────────────────────────────────────────────────────────────
// Processor: 포인트 정산 (10% 공제)
// ─────────────────────────────────────────────────────────────────────────────

@Injectable()
export class UserPointProcessor implements ItemProcessor<User, SettledUser> {
  async process(items: User[]): Promise<SettledUser[]> {
    return items.map((user) => ({
      id: user.id,
      name: user.name,
      finalPoints: Math.floor(user.rawPoints * 0.9), // 10% 공제
      settledAt: new Date(),
    }));
  }
}

// ─────────────────────────────────────────────────────────────────────────────
// Writer: 콘솔에 출력 (실제 앱에서는 DB 저장)
// ─────────────────────────────────────────────────────────────────────────────

@Injectable()
export class UserPointWriter implements ItemWriter<SettledUser> {
  private totalWritten = 0;

  async write(items: SettledUser[]): Promise<void> {
    this.totalWritten += items.length;
    console.log(
      `[Writer] Settled ${items.length} users ` +
        `(total: ${this.totalWritten}) — ` +
        `IDs: ${items.map((u) => u.id).join(', ')}`,
    );
  }
}

// ─────────────────────────────────────────────────────────────────────────────
// Job 클래스
// ─────────────────────────────────────────────────────────────────────────────

/**
 * 사용자 포인트 정산 배치 Job.
 *
 * 전체 유저를 20명씩 읽어 포인트를 정산(10% 공제)하고
 * 결과를 기록하는 단일 Step Job.
 */
@Job({
  name: 'user-point-settlement',
  description: '사용자 포인트 정산 (10% 세금 공제)',
  preventDuplicateRun: true,
})
export class UserPointSettlementJob {
  constructor(
    private readonly reader: UserPointReader,
    private readonly processor: UserPointProcessor,
    private readonly writer: UserPointWriter,
  ) {}

  @Step({ chunkSize: 20, order: 1, retryLimit: 3 })
  settlePoints(): StepDefinition {
    return {
      name: 'settle-points',
      chunkSize: 20,
      reader: this.reader,
      processor: this.processor,
      writer: this.writer,
    };
  }
}
