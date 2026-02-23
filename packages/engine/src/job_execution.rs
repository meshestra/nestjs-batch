use chrono::Utc;
use napi_derive::napi;
use serde::{Deserialize, Serialize};

/// 배치 Job의 실행 상태를 나타내는 열거형
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum JobStatus {
    /// 실행 대기 중
    Idle,
    /// 실행 시작됨
    Started,
    /// 정상 완료
    Completed,
    /// 실패 (에러 메시지 포함)
    Failed(String),
    /// 외부 요청에 의해 중단됨
    Stopped,
    /// 이전 실행에서 중단된 후 재시작됨
    Restarted,
}

impl std::fmt::Display for JobStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JobStatus::Idle => write!(f, "IDLE"),
            JobStatus::Started => write!(f, "STARTED"),
            JobStatus::Completed => write!(f, "COMPLETED"),
            JobStatus::Failed(msg) => write!(f, "FAILED: {}", msg),
            JobStatus::Stopped => write!(f, "STOPPED"),
            JobStatus::Restarted => write!(f, "RESTARTED"),
        }
    }
}

impl From<&str> for JobStatus {
    fn from(s: &str) -> Self {
        match s {
            "IDLE" => JobStatus::Idle,
            "STARTED" => JobStatus::Started,
            "COMPLETED" => JobStatus::Completed,
            "STOPPED" => JobStatus::Stopped,
            "RESTARTED" => JobStatus::Restarted,
            s if s.starts_with("FAILED") => {
                let msg = s.trim_start_matches("FAILED: ").to_string();
                JobStatus::Failed(msg)
            }
            _ => JobStatus::Failed(format!("Unknown status: {}", s)),
        }
    }
}

/// Step 실행의 세부 통계
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepExecution {
    /// Step 이름
    pub step_name: String,
    /// Step 상태
    pub status: String,
    /// 읽은 총 아이템 수
    pub read_count: i64,
    /// 처리(프로세싱)한 총 아이템 수
    pub process_count: i64,
    /// 기록(쓰기)한 총 아이템 수
    pub write_count: i64,
    /// 읽기 오류로 건너뛴 아이템 수
    pub read_skip_count: i64,
    /// 처리 오류로 건너뛴 아이템 수
    pub process_skip_count: i64,
    /// 쓰기 오류로 건너뛴 아이템 수
    pub write_skip_count: i64,
    /// 완료된 Chunk 수
    pub commit_count: i64,
    /// 롤백된 Chunk 수
    pub rollback_count: i64,
    /// Step 시작 시각 (Unix ms)
    pub start_time: i64,
    /// Step 종료 시각 (Unix ms), 실행 중이면 None
    pub end_time: Option<i64>,
    /// 실행 소요 시간 (ms)
    pub duration_ms: Option<i64>,
}

impl StepExecution {
    /// 새로운 StepExecution을 초기화한다
    pub fn new(step_name: impl Into<String>) -> Self {
        Self {
            step_name: step_name.into(),
            status: JobStatus::Started.to_string(),
            read_count: 0,
            process_count: 0,
            write_count: 0,
            read_skip_count: 0,
            process_skip_count: 0,
            write_skip_count: 0,
            commit_count: 0,
            rollback_count: 0,
            start_time: Utc::now().timestamp_millis(),
            end_time: None,
            duration_ms: None,
        }
    }

    /// Step이 완료되면 종료 시각과 소요 시간을 기록한다
    pub fn complete(&mut self, status: JobStatus) {
        let end = Utc::now().timestamp_millis();
        self.status = status.to_string();
        self.end_time = Some(end);
        self.duration_ms = Some(end - self.start_time);
    }

    /// 총 건너뛴 아이템 수를 반환한다
    pub fn total_skip_count(&self) -> i64 {
        self.read_skip_count + self.process_skip_count + self.write_skip_count
    }
}

/// NAPI에 노출되는 Job 실행 결과 객체
///
/// JS 쪽에서 `JobLauncher.launch()` 호출 결과로 전달받는 구조체.
#[napi(object)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobExecution {
    /// 실행된 Job의 고유 ID (UUID v4)
    pub job_execution_id: String,
    /// Job 이름
    pub job_name: String,
    /// 최종 상태 문자열 ("COMPLETED", "FAILED", ...)
    pub status: String,
    /// Job 전체 읽기 수 (모든 Step 합계)
    pub read_count: i64,
    /// Job 전체 처리 수 (모든 Step 합계)
    pub process_count: i64,
    /// Job 전체 쓰기 수 (모든 Step 합계)
    pub write_count: i64,
    /// Job 전체 건너뛴 수 (모든 Step 합계)
    pub skip_count: i64,
    /// 완료된 Chunk 커밋 수 (모든 Step 합계)
    pub commit_count: i64,
    /// Job 시작 시각 (Unix ms)
    pub start_time: i64,
    /// Job 종료 시각 (Unix ms), 실행 중이면 None
    pub end_time: Option<i64>,
    /// 전체 소요 시간 (ms)
    pub duration_ms: Option<i64>,
    /// 종료 메시지 (오류가 있을 경우 오류 원인 포함)
    pub exit_message: Option<String>,
    /// 각 Step의 세부 실행 결과 (JSON 직렬화)
    pub step_executions: Vec<String>,
}

impl JobExecution {
    /// 새로운 JobExecution을 생성한다
    pub fn new(job_name: impl Into<String>) -> Self {
        let id = uuid_v4();
        Self {
            job_execution_id: id,
            job_name: job_name.into(),
            status: JobStatus::Started.to_string(),
            read_count: 0,
            process_count: 0,
            write_count: 0,
            skip_count: 0,
            commit_count: 0,
            start_time: Utc::now().timestamp_millis(),
            end_time: None,
            duration_ms: None,
            exit_message: None,
            step_executions: Vec::new(),
        }
    }

    /// StepExecution의 통계를 Job 레벨로 누적한다
    pub fn accumulate_step(&mut self, step: &StepExecution) {
        self.read_count += step.read_count;
        self.process_count += step.process_count;
        self.write_count += step.write_count;
        self.skip_count += step.total_skip_count();
        self.commit_count += step.commit_count;

        // Step 결과를 JSON으로 직렬화해서 보관
        if let Ok(json) = serde_json::to_string(step) {
            self.step_executions.push(json);
        }
    }

    /// Job을 성공으로 완료 처리한다
    pub fn complete(&mut self) {
        let end = Utc::now().timestamp_millis();
        self.status = JobStatus::Completed.to_string();
        self.end_time = Some(end);
        self.duration_ms = Some(end - self.start_time);
    }

    /// Job을 실패로 완료 처리한다
    pub fn fail(&mut self, reason: impl Into<String>) {
        let reason = reason.into();
        let end = Utc::now().timestamp_millis();
        self.status = JobStatus::Failed(reason.clone()).to_string();
        self.exit_message = Some(reason);
        self.end_time = Some(end);
        self.duration_ms = Some(end - self.start_time);
    }

    /// Job을 중단 처리한다
    pub fn stop(&mut self, reason: Option<String>) {
        let end = Utc::now().timestamp_millis();
        self.status = JobStatus::Stopped.to_string();
        self.exit_message = reason;
        self.end_time = Some(end);
        self.duration_ms = Some(end - self.start_time);
    }

    /// 현재 Job이 실행 중인지 여부
    pub fn is_running(&self) -> bool {
        self.status == JobStatus::Started.to_string()
    }
}

/// 간단한 UUID v4 생성기 (외부 크레이트 없이 rand 사용)
///
/// 실제 프로덕션에서는 `uuid` 크레이트를 사용하는 것을 권장한다.
fn uuid_v4() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);

    // 간소화된 UUID 형식 생성 (데모용)
    format!(
        "{:08x}-{:04x}-4{:03x}-{:04x}-{:012x}",
        nanos,
        (nanos >> 16) & 0xFFFF,
        (nanos >> 4) & 0xFFF,
        0x8000 | ((nanos >> 2) & 0x3FFF),
        nanos as u64 * 0x1000193
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_job_status_display() {
        assert_eq!(JobStatus::Started.to_string(), "STARTED");
        assert_eq!(JobStatus::Completed.to_string(), "COMPLETED");
        assert_eq!(
            JobStatus::Failed("disk full".into()).to_string(),
            "FAILED: disk full"
        );
    }

    #[test]
    fn test_job_status_from_str() {
        assert_eq!(JobStatus::from("STARTED"), JobStatus::Started);
        assert_eq!(JobStatus::from("COMPLETED"), JobStatus::Completed);
        assert_eq!(
            JobStatus::from("FAILED: disk full"),
            JobStatus::Failed("disk full".into())
        );
    }

    #[test]
    fn test_step_execution_lifecycle() {
        let mut step = StepExecution::new("userReadStep");
        assert_eq!(step.status, "STARTED");
        step.read_count = 1000;
        step.write_count = 1000;
        step.commit_count = 10;
        step.complete(JobStatus::Completed);
        assert_eq!(step.status, "COMPLETED");
        assert!(step.end_time.is_some());
        assert!(step.duration_ms.unwrap() >= 0);
    }

    #[test]
    fn test_job_execution_accumulation() {
        let mut job = JobExecution::new("migrationJob");
        let mut step = StepExecution::new("step1");
        step.read_count = 500;
        step.process_count = 500;
        step.write_count = 490;
        step.read_skip_count = 10;
        step.commit_count = 5;
        step.complete(JobStatus::Completed);

        job.accumulate_step(&step);

        assert_eq!(job.read_count, 500);
        assert_eq!(job.write_count, 490);
        assert_eq!(job.skip_count, 10);
        assert_eq!(job.commit_count, 5);
        assert_eq!(job.step_executions.len(), 1);
    }

    #[test]
    fn test_job_execution_fail() {
        let mut job = JobExecution::new("failJob");
        job.fail("Connection timeout");
        assert!(job.status.starts_with("FAILED"));
        assert_eq!(job.exit_message, Some("Connection timeout".to_string()));
        assert!(job.end_time.is_some());
    }
}
