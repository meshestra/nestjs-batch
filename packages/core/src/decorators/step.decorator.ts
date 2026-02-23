import { StepDecoratorOptions } from '../interfaces';
import { STEP_METADATA_KEY, STEP_METHODS_METADATA_KEY } from './constants';

/**
 * Job 클래스의 메서드를 Step으로 등록하는 메서드 데코레이터.
 *
 * `@Step()`이 붙은 메서드는 `StepDefinition`을 반환해야 한다.
 * `BatchRegistry`가 앱 시작 시 해당 메서드를 호출하여 Step 정의를 수집한다.
 *
 * @param options Step 설정 옵션
 *
 * @example
 * ```typescript
 * @Job({ name: 'user-migration' })
 * export class UserMigrationJob {
 *   constructor(private reader: UserReader, private writer: MemberWriter) {}
 *
 *   @Step({ chunkSize: 500, order: 1 })
 *   migrateStep(): StepDefinition {
 *     return {
 *       name: 'migrate-users',
 *       chunkSize: 500,
 *       reader: this.reader,
 *       processor: new UserToMemberProcessor(),
 *       writer: this.writer,
 *     };
 *   }
 * }
 * ```
 */
export function Step(options: StepDecoratorOptions = {}): MethodDecorator {
  return (
    target: object,
    propertyKey: string | symbol,
    descriptor: PropertyDescriptor,
  ) => {
    // 개별 메서드에 Step 메타데이터 부착
    Reflect.defineMetadata(STEP_METADATA_KEY, options, target, propertyKey);

    // 클래스 프로토타입에 Step 메서드 목록 누적
    const existingMethods: (string | symbol)[] =
      Reflect.getOwnMetadata(STEP_METHODS_METADATA_KEY, target) ?? [];
    if (!existingMethods.includes(propertyKey)) {
      existingMethods.push(propertyKey);
    }
    Reflect.defineMetadata(STEP_METHODS_METADATA_KEY, existingMethods, target);

    return descriptor;
  };
}
