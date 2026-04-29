use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronJob {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub schedule: String,
    pub timezone: String,
    pub user_id: String,
    pub channel_id: Option<String>,
    pub template: String,
    pub prompt: String,
    pub enabled: bool,
    pub trigger_type: String,
    pub last_run: Option<String>,
    pub next_run: Option<String>,
    pub run_count: i64,
    pub last_error: Option<String>,
}

pub struct CronScheduler {
    jobs: HashMap<String, CronJob>,
}

impl CronScheduler {
    pub fn new() -> Self {
        Self {
            jobs: HashMap::new(),
        }
    }

    pub fn add_job(&mut self, job: CronJob) {
        self.jobs.insert(job.id.clone(), job);
    }

    pub fn remove_job(&mut self, id: &str) -> Option<CronJob> {
        self.jobs.remove(id)
    }

    pub fn get_job(&self, id: &str) -> Option<&CronJob> {
        self.jobs.get(id)
    }

    pub fn list_jobs(&self) -> Vec<&CronJob> {
        self.jobs.values().collect()
    }

    pub fn enabled_jobs(&self) -> Vec<&CronJob> {
        self.jobs.values().filter(|j| j.enabled).collect()
    }

    pub fn validate_schedule(schedule: &str) -> bool {
        cron::Schedule::from_str(schedule).is_ok()
    }
}

use std::str::FromStr;

#[cfg(test)]
mod cron_tests {
    use super::*;

    #[test]
    fn test_cron_scheduler_add_remove() {
        let mut scheduler = CronScheduler::new();
        let job = CronJob {
            id: "test1".to_string(),
            name: "Test Job".to_string(),
            description: None,
            schedule: "0 * * * * *".to_string(),
            timezone: "UTC".to_string(),
            user_id: "user1".to_string(),
            channel_id: None,
            template: "test.poml".to_string(),
            prompt: "Do something".to_string(),
            enabled: true,
            trigger_type: "cron".to_string(),
            last_run: None,
            next_run: None,
            run_count: 0,
            last_error: None,
        };

        scheduler.add_job(job);
        assert!(scheduler.get_job("test1").is_some());
        assert_eq!(scheduler.list_jobs().len(), 1);

        scheduler.remove_job("test1");
        assert!(scheduler.get_job("test1").is_none());
    }

    #[test]
    fn test_cron_scheduler_enabled_jobs() {
        let mut scheduler = CronScheduler::new();

        scheduler.add_job(CronJob {
            id: "j1".to_string(),
            name: "Job 1".to_string(),
            description: None,
            schedule: "0 * * * * *".to_string(),
            timezone: "UTC".to_string(),
            user_id: "user1".to_string(),
            channel_id: None,
            template: "t.poml".to_string(),
            prompt: "p".to_string(),
            enabled: true,
            trigger_type: "cron".to_string(),
            last_run: None,
            next_run: None,
            run_count: 0,
            last_error: None,
        });

        scheduler.add_job(CronJob {
            id: "j2".to_string(),
            name: "Job 2".to_string(),
            description: None,
            schedule: "0 * * * * *".to_string(),
            timezone: "UTC".to_string(),
            user_id: "user1".to_string(),
            channel_id: None,
            template: "t.poml".to_string(),
            prompt: "p".to_string(),
            enabled: false,
            trigger_type: "cron".to_string(),
            last_run: None,
            next_run: None,
            run_count: 0,
            last_error: None,
        });

        assert_eq!(scheduler.enabled_jobs().len(), 1);
    }

    #[test]
    fn test_validate_schedule() {
        assert!(CronScheduler::validate_schedule("0 * * * * *"));
        assert!(CronScheduler::validate_schedule("0 0 * * * *"));
        assert!(!CronScheduler::validate_schedule("invalid"));
    }
}
