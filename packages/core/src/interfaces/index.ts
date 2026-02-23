/**
 * @nestjs-batch/core - 핵심 인터페이스 모음
 *
 * Spring Batch의 Reader → Processor → Writer 패턴을 TypeScript 로 모델링한다.
 * 사용자는 이 인터페이스를 구현하여 배치 Job의 비즈니스 로직을 작성한다.
 */

// ─────────────────────────────────────────────────────────────────────────────
// Item 처리 인터페이스
// ─────────────────────────────────────────────────────────────────────────────

/**
 * 데이터 소스에서 아이템을 읽어오는 Reader 인터페이스.
 *
 * Rust 엔진이 각 Chunk 시작 시 이 함수를 호출한다.
 *
 * @template O - Reader가 반환하는 아이템의 타입
 *
 * @example
 * ```typescript
 * class UserReader implements ItemReader<User> {
 *   constructor(private readonly userRepo: UserRepository) {}
 *
 *   async read(offset: number, chunkSize: number): Promise<User[] | null> {
 *     const users = await this.userRepo.findMany({
 *       skip: offset,
 *       take: chunkSize,
 *     });
 *     return users.length > 0 ? users : null;
 *   }
 * }
 * ```
 */
export interface ItemReader<O = unknown> {
  /**
   * 데이터 소스에서 한 Chunk 분량의 아이템을 읽어온다.
   *
   * @param offset    현재까지 읽은 누적 아이템 수 (페이지네이션 커서)
   * @param chunkSize 이번 Chunk에서 읽어야 할 최대 아이템 수
   * @returns 아이템 배열, 더 이상 읽을 데이터가 없으면 `null` 또는 빈 배열
   */
  read(offset: number, chunkSize: number): Promise<O[] | null>;
}

/**
 * 아이템을 변환·필터링하는 Processor 인터페이스.
 *
 * Reader가 반환한 아이템 배열을 받아 처리 후 새로운 배열을 반환한다.
 * 특정 아이템을 건너뛰려면 해당 아이템을 결과 배열에서 제외하면 된다.
 *
 * @template I - 입력 아이템 타입 (Reader의 출력 타입과 일치해야 함)
 * @template O - 출력 아이템 타입 (Writer의 입력 타입과 일치해야 함)
 *
 * @example
 * ```typescript
 * class UserToMemberProcessor implements ItemProcessor<User, Member> {
 *   async process(items: User[]): Promise<Member[]> {
 *     return items
 *       .filter(u => u.isActive)
 *       .map(u => ({ id: u.id, name: u.name, joinedAt: new Date() }));
 *   }
 * }
 * ```
 */
export interface ItemProcessor<I = unknown, O = unknown> {
  /**
   * 한 Chunk 분량의 아이템을 처리하여 변환된 배열을 반환한다.
   *
   * @param items Reader에서 읽어온 아이템 배열
   * @returns 변환·필터링된 아이템 배열
   */
  process(items: I[]): Promise<O[]>;
}

/**
 * 처리된 아이템을 대상 저장소에 기록하는 Writer 인터페이스.
 *
 * Processor가 반환한 아이템 배열을 받아 DB, 파일, 메시지 큐 등에 기록한다.
 * Writer 호출이 성공하면 해당 Chunk가 커밋된 것으로 간주한다.
 *
 * @template I - Writer가 받는 아이템 타입 (Processor의 출력 타입과 일치해야 함)
 *
 * @example
 * ```typescript
 * class MemberWriter implements ItemWriter<Member> {
 *   constructor(private readonly memberRepo: MemberRepository) {}
 *
 *   async write(items: Member[]): Promise<void> {
 *     await this.memberRepo.bulkInsert(items);
 *   }
 * }
 * ```
 */
export interface ItemWriter<I = unknown> {
  /**
   * 한 Chunk 분량의 아이템을 대상 저장소에 기록한다.
   *
   * @param items Processor에서 변환된 아이템 배열
   */
  write(items: I[]): Promise<void>;
}

// ─────────────────────────────────────────────────────────────────────────────
// Job 실행 상태 타입
// ─────────────────────────────────────────────────────────────────────────────

/**
 * Job 실행 상태를 나타내는 유니온 타입.
 *
 * - `IDLE`      : 등록만 되고 아직 실행되지 않은 상태
 * - `STARTED`   : 실행 중
 * - `COMPLETED` : 정상 완료
 * - `FAILED`    : 오류로 인한 실패
 * - `STOPPED`   : 외부 요청으로 중단됨
 * - `RESTARTED` : 이전에 중단된 Job이 재시작됨
 */
export type JobStatusType =
  | 'IDLE'
  | 'STARTED'
  | 'COMPLETED'
  | 'FAILED'
  | 'STOPPED'
  | 'RESTARTED';

/**
 * Job 실행 파라미터.
 * JobLauncher.launch() 호출 시 전달하며, JobRepository에 함께 저장된다.
 */
export type JobParameters = Record<string, string | number | boolean | Date>;

/**
 * Step 실행 결과의 세부 통계.
 * Rust 엔진의 `StepExecution` 구조체와 대응한다.
 */
export interface StepExecutionRecord {
  stepName: string;
  status: JobStatusType;
  readCount: number;
  processCount: number;
  writeCount: number;
  readSkipCount: number;
  processSkipCount: number;
  writeSkipCount: number;
  commitCount: number;
  rollbackCount: number;
  startTime: number;
  endTime: number | null;
  durationMs: number | null;
}

/**
 * Job 실행의 전체 결과.
 * Rust 엔진의 `JobExecution` 구조체와 대응하며 JobRepository에 저장된다.
 */
export interface JobExecutionRecord {
  /** 고유 실행 ID */
  jobExecutionId: string;
  /** Job 이름 */
  jobName: string;
  /** Job 파라미터 */
  jobParameters: JobParameters;
  /** 실행 상태 */
  status: JobStatusType;
  /** 전체 읽기 수 */
  readCount: number;
  /** 전체 처리 수 */
  processCount: number;
  /** 전체 쓰기 수 */
  writeCount: number;
  /** 전체 건너뛴 수 */
  skipCount: number;
  /** 커밋 수 */
  commitCount: number;
  /** 시작 시각 (Unix ms) */
  startTime: number;
  /** 종료 시각 (Unix ms) */
  endTime: number | null;
  /** 소요 시간 (ms) */
  durationMs: number | null;
  /** 종료 메시지 (오류 원인 등) */
  exitMessage: string | null;
  /** 각 Step의 상세 실행 결과 */
  stepExecutions: StepExecutionRecord[];
}

// ─────────────────────────────────────────────────────────────────────────────
// JobRepository 인터페이스
// ─────────────────────────────────────────────────────────────────────────────

/**
 * Job 실행 이력을 저장하고 조회하는 Repository 인터페이스.
 *
 * 기본 구현체는 인메모리(`InMemoryJobRepository`)가 제공되며,
 * 사용자는 이 인터페이스를 구현하여 DB 기반 Repository로 교체할 수 있다.
 *
 * @example DB 기반 커스텀 구현
 * ```typescript
 * @Injectable()
 * class PrismaJobRepository implements JobRepository {
 *   constructor(private readonly prisma: PrismaService) {}
 *
 *   async save(record: JobExecutionRecord): Promise<void> {
 *     await this.prisma.jobExecution.upsert({
 *       where: { jobExecutionId: record.jobExecutionId },
 *       create: record,
 *       update: record,
 *     });
 *   }
 *   // ...
 * }
 * ```
 */
export interface JobRepository {
  /**
   * Job 실행 결과를 저장하거나 업데이트한다.
   * 동일한 `jobExecutionId`가 있으면 덮어쓴다.
   *
   * @param record 저장할 실행 기록
   */
  save(record: JobExecutionRecord): Promise<void>;

  /**
   * 실행 ID로 특정 실행 결과를 조회한다.
   *
   * @param jobExecutionId 조회할 실행 ID
   * @returns 실행 기록, 없으면 `null`
   */
  findById(jobExecutionId: string): Promise<JobExecutionRecord | null>;

  /**
   * 특정 Job의 전체 실행 이력을 최신 순으로 조회한다.
   *
   * @param jobName   조회할 Job 이름
   * @param limit     최대 반환 개수 (기본값: 20)
   * @returns 실행 기록 배열 (최신 순)
   */
  findAllByJobName(
    jobName: string,
    limit?: number,
  ): Promise<JobExecutionRecord[]>;

  /**
   * 특정 Job의 가장 최근 실행 결과를 반환한다.
   *
   * @param jobName 조회할 Job 이름
   * @returns 가장 최근 실행 기록, 없으면 `null`
   */
  findLastExecution(jobName: string): Promise<JobExecutionRecord | null>;

  /**
   * 특정 상태의 실행 기록을 전부 반환한다.
   * 중단된 Job을 재시작하거나 실패한 Job 목록을 조회할 때 활용한다.
   *
   * @param status 조회할 상태
   * @returns 해당 상태의 실행 기록 배열
   */
  findByStatus(status: JobStatusType): Promise<JobExecutionRecord[]>;

  /**
   * 특정 Job이 현재 실행 중인지 확인한다.
   * 중복 실행 방지 로직에서 활용한다.
   *
   * @param jobName 확인할 Job 이름
   * @returns 실행 중이면 `true`
   */
  isRunning(jobName: string): Promise<boolean>;

  /**
   * 특정 실행 기록의 상태를 업데이트한다.
   *
   * @param jobExecutionId 업데이트할 실행 ID
   * @param status         새로운 상태
   * @param exitMessage    종료 메시지 (선택)
   */
  updateStatus(
    jobExecutionId: string,
    status: JobStatusType,
    exitMessage?: string,
  ): Promise<void>;

  /**
   * 보관 기간이 지난 실행 기록을 삭제한다 (선택적 구현).
   * 인메모리 구현에서는 단순히 전체 초기화로 대체할 수 있다.
   *
   * @param olderThanMs 이 시각(Unix ms)보다 오래된 기록을 삭제
   * @returns 삭제된 기록 수
   */
  purgeOlderThan?(olderThanMs: number): Promise<number>;
}

// ─────────────────────────────────────────────────────────────────────────────
// Step 정의 인터페이스
// ─────────────────────────────────────────────────────────────────────────────

/**
 * Step 하나를 정의하는 메타데이터.
 * `@Step()` 데코레이터와 `BatchRegistry`에서 사용된다.
 */
export interface StepDefinition {
  /** Step 이름 (Job 내에서 유일해야 함) */
  name: string;
  /** Chunk 크기 */
  chunkSize: number;
  /** Reader 인스턴스 또는 팩토리 함수 */
  reader: ItemReader<unknown>;
  /** Processor 인스턴스 또는 팩토리 함수 */
  processor: ItemProcessor<unknown, unknown>;
  /** Writer 인스턴스 또는 팩토리 함수 */
  writer: ItemWriter<unknown>;
  /** 건너뛸 수 있는 최대 아이템 수 */
  skipLimit?: number;
  /** Writer 실패 시 재시도 횟수 */
  retryLimit?: number;
  /** 이 Step 이후에 실행할 다음 Step 이름 (선형 체이닝) */
  nextStep?: string;
}

/**
 * Job 전체를 정의하는 메타데이터.
 * `@Job()` 데코레이터와 `BatchRegistry`에서 사용된다.
 */
export interface JobDefinition {
  /** Job 이름 (시스템 내에서 유일해야 함) */
  name: string;
  /** 이 Job에 속한 Step 목록 (실행 순서대로) */
  steps: StepDefinition[];
  /** 중복 실행 방지 여부 (기본값: true) */
  preventDuplicateRun?: boolean;
  /** Job 설명 (문서화용) */
  description?: string;
}

// ─────────────────────────────────────────────────────────────────────────────
// JobLauncher 인터페이스
// ─────────────────────────────────────────────────────────────────────────────

/**
 * Job 실행을 시작하는 런처 인터페이스.
 */
export interface IJobLauncher {
  /**
   * 등록된 Job을 실행한다.
   *
   * @param jobName       실행할 Job 이름
   * @param jobParameters Job 실행 파라미터 (선택)
   * @returns Job 실행 결과
   */
  launch(
    jobName: string,
    jobParameters?: JobParameters,
  ): Promise<JobExecutionRecord>;

  /**
   * 실행 중인 Job에 중단 신호를 보낸다.
   *
   * @param jobExecutionId 중단할 실행 ID
   */
  stop(jobExecutionId: string): Promise<void>;
}

// ─────────────────────────────────────────────────────────────────────────────
// 데코레이터 옵션 타입
// ─────────────────────────────────────────────────────────────────────────────

/**
 * `@Job()` 데코레이터 옵션
 */
export interface JobDecoratorOptions {
  /** Job 이름. 생략 시 클래스 이름에서 자동 유도 */
  name?: string;
  /** Job 설명 */
  description?: string;
  /** 중복 실행 방지 여부 (기본값: true) */
  preventDuplicateRun?: boolean;
}

/**
 * `@Step()` 데코레이터 옵션
 */
export interface StepDecoratorOptions {
  /** Step 이름. 생략 시 메서드 이름에서 자동 유도 */
  name?: string;
  /** 한 번의 Chunk에서 처리할 최대 아이템 수 (기본값: 100) */
  chunkSize?: number;
  /** 건너뛸 수 있는 최대 아이템 수 (기본값: 0 — 건너뛰기 비활성화) */
  skipLimit?: number;
  /** Writer 실패 시 재시도 횟수 (기본값: 3) */
  retryLimit?: number;
  /** 실행 순서 (낮을수록 먼저 실행, 기본값: 0) */
  order?: number;
}
