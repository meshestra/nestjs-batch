import { BatchModule } from '@nestjs-batch/core';
import { Module } from '@nestjs/common';
import {
  UserPointProcessor,
  UserPointReader,
  UserPointSettlementJob,
  UserPointWriter,
} from './jobs/user-point-settlement';

/**
 * 샘플 앱 루트 모듈.
 *
 * BatchModule.forRoot() 를 등록하고,
 * Job 클래스와 Reader/Processor/Writer를 Provider로 등록한다.
 *
 * BatchModule은 global: true로 설정되어 있어
 * JobLauncher와 BatchRegistry를 어디서든 주입받을 수 있다.
 */
@Module({
  imports: [
    // 기본 설정: InMemoryJobRepository 사용
    BatchModule.forRoot({}),
  ],
  providers: [
    // Job 의존 컴포넌트
    UserPointReader,
    UserPointProcessor,
    UserPointWriter,
    // Job 클래스 (@Job 데코레이터로 자동 Injectable 처리됨)
    UserPointSettlementJob,
  ],
})
export class AppModule { }
