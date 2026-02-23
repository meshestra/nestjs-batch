/**
 * Reflect.metadata 키 상수 모음
 *
 * @Job() / @Step() 데코레이터가 클래스/메서드에 부착하는 메타데이터 키.
 * 프레임워크 내부에서만 사용하며 외부로 노출하지 않는다.
 */
export const JOB_METADATA_KEY = Symbol('nestjs-batch:job');
export const STEP_METADATA_KEY = Symbol('nestjs-batch:step');

/** @Step() 데코레이터가 붙은 메서드 목록을 저장하는 키 */
export const STEP_METHODS_METADATA_KEY = Symbol('nestjs-batch:step-methods');
