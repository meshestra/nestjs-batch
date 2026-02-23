# JobParameters Late Binding 설계

## 문제 정의

현재 `BatchRegistry`는 모듈 초기화(`OnModuleInit`) 시점에 `@Step()` 메서드를 **즉시 호출**하여 `StepDefinition`을 고정한다.

```
AppModule 초기화
  └─ BatchRegistry.onModuleInit()
       └─ collectSteps()
            └─ instance[methodKey]()  ← 지금 여기서 StepDefinition 확정
                 └─ { reader, processor, writer } 인스턴스 고정
```

`JobLauncher.launch('settleJob', { targetDate: '2025-01-15' })`를 호출해도 이미 굳어진 `StepDefinition`에 `jobParameters`가 전달될 경로가 없다.

```typescript
// job.launcher.ts:86 — jobParameters가 runStep으로 흘러가지 않는다
for (const stepDef of jobDef.steps) {
  const stepRecord = await this.runStep(jobExecutionId, jobName, stepDef);
  //                                                            ^^^^^^
  //                                  jobParameters 없음
}
```

---

## 해결 방향: Step Factory 패턴

`StepDefinition`을 미리 만들어두는 대신, **실행 시점에 `jobParameters`를 받아 `StepDefinition`을 생성하는 팩토리 함수**로 전환한다.

### 핵심 아이디어

```
현재:  @Step 메서드 → StepDefinition (모듈 초기화 시 1회 실행, 고정)
변경:  @Step 메서드 → (params) => StepDefinition (실행마다 호출, 동적 생성)
```

---

## 설계

### 1. StepFactory 타입 추가

```typescript
// interfaces/index.ts

export type StepFactory = (params: JobParameters) => StepDefinition;

export interface JobDefinition {
  name: string;
  steps: StepFactory[];          // StepDefinition[] → StepFactory[]
  preventDuplicateRun?: boolean;
  description?: string;
}
```

### 2. @Step 메서드 시그니처 변경

사용자는 `params`를 받아 `StepDefinition`을 반환하도록 작성한다.

```typescript
// 변경 전
@Step({ chunkSize: 500, order: 1 })
settleStep(): StepDefinition {
  return { reader: this.reader, processor: this.processor, writer: this.writer };
}

// 변경 후
@Step({ chunkSize: 500, order: 1 })
settleStep(params: JobParameters): StepDefinition {
  return {
    name: 'settle-step',
    chunkSize: 500,
    reader: this.reader,
    processor: this.processor,
    writer: this.writer,
  };
  // reader 내부에서 params.targetDate 를 생성자/setter 없이 직접 사용 가능하게
  // → Reader도 팩토리로 만들거나, params를 StepDefinition에 함께 전달
}
```

Reader/Writer가 `params`에 접근하려면 `StepDefinition`에 `params`를 포함시켜 전달한다.

```typescript
export interface StepDefinition {
  name: string;
  chunkSize: number;
  reader: ItemReader<unknown>;
  processor: ItemProcessor<unknown, unknown>;
  writer: ItemWriter<unknown>;
  skipLimit?: number;
  retryLimit?: number;
  params?: JobParameters;        // ← 추가
}
```

Reader는 `read()` 시그니처 대신 **생성 시점에 params를 받는 팩토리**로 만들거나, Step 메서드 안에서 직접 params를 클로저로 캡처한다.

```typescript
// 클로저 캡처 방식 (가장 단순)
@Step({ chunkSize: 500, order: 1 })
settleStep(params: JobParameters): StepDefinition {
  const targetDate = params.targetDate as string;

  return {
    name: 'settle-step',
    chunkSize: 500,
    reader: {
      read: (offset, chunkSize) =>
        this.orderRepository.findDelivered(targetDate, offset, chunkSize),
    },
    processor: this.processor,
    writer: this.writer,
  };
}
```

### 3. BatchRegistry 변경 — 팩토리 저장

`collectSteps()`가 메서드를 호출하지 않고 **팩토리 함수 자체를 저장**한다.

```typescript
// batch.registry.ts — 변경 전
stepDef = instance[methodKey]();           // 즉시 실행

// 변경 후
const factory: StepFactory =
  (params: JobParameters) => instance[methodKey](params);  // 팩토리로 래핑
stepsWithOrder.push({ factory, order: stepOptions.order ?? 0 });
```

### 4. JobLauncher 변경 — 실행 시 팩토리 호출

```typescript
// job.launcher.ts — 변경 전
for (const stepDef of jobDef.steps) {
  const stepRecord = await this.runStep(jobExecutionId, jobName, stepDef);
}

// 변경 후
for (const stepFactory of jobDef.steps) {
  const stepDef = stepFactory(jobParameters);   // ← 실행마다 params 주입
  const stepRecord = await this.runStep(jobExecutionId, jobName, stepDef);
}
```

---

## 변경 범위 요약

| 파일 | 변경 내용 |
|------|----------|
| `interfaces/index.ts` | `StepFactory` 타입 추가, `JobDefinition.steps` 타입 변경, `StepDefinition.params` 필드 추가 |
| `registry/batch.registry.ts` | `collectSteps()`에서 즉시 호출 → 팩토리 래핑으로 변경 |
| `launcher/job.launcher.ts` | Step 루프에서 `stepFactory(jobParameters)` 호출로 변경 |
| 사용자 Job 클래스 | `@Step` 메서드 시그니처에 `params: JobParameters` 추가 (선택적 — 없어도 동작) |

---

## 호환성

- `@Step` 메서드에서 `params`를 사용하지 않으면 기존 코드 그대로 동작한다.

```typescript
// params 무시 → 기존 방식과 동일하게 동작
@Step({ chunkSize: 20 })
myStep(_params: JobParameters): StepDefinition {
  return { reader: this.reader, processor: this.processor, writer: this.writer };
}
```

- `JobLauncher.launch()` 시그니처는 변경 없다.

---

## 실행 흐름 (변경 후)

```
jobLauncher.launch('daily-settlement', { targetDate: '2025-01-15' })
  │
  ├─ JobRepository.isRunning() 체크
  │
  ├─ for each stepFactory in jobDef.steps:
  │    │
  │    ├─ stepDef = stepFactory({ targetDate: '2025-01-15' })
  │    │    └─ DailySettlementJob.settleStep({ targetDate: '2025-01-15' })
  │    │         └─ targetDate 클로저 캡처 → reader/writer에 전달
  │    │
  │    └─ runStep(jobExecutionId, jobName, stepDef)
  │         └─ Rust ChunkExecutor: read → process → write 루프
  │
  └─ JobExecutionRecord 저장 및 반환
```

---

## 구현 순서

1. `interfaces/index.ts` — `StepFactory` 타입, `StepDefinition.params` 추가
2. `registry/batch.registry.ts` — 팩토리 래핑으로 변경
3. `launcher/job.launcher.ts` — 팩토리 호출로 변경
4. `examples/ecommerce-settlement-batch` — Late Binding 활용 예제 적용
