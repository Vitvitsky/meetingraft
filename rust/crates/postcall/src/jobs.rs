//! Фоновый пересбор Final: состояние, прогресс, отмена (Phase 10, T6).
//!
//! Проход идёт минутами, а граница UniFFI синхронная — значит работа
//! уезжает в поток, а наружу торчит только состояние.
//!
//! Запуск потока вынесен за трейт `Spawner` намеренно: с реальными
//! потоками тесты на состояние стали бы гонкой со `sleep`, а так они
//! синхронные и детерминированные.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Стадия пересбора.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RebuildState {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

impl RebuildState {
    pub fn code(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }
}

/// Снимок состояния для UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RebuildProgress {
    pub job_id: String,
    pub meeting_id: String,
    pub state: RebuildState,
    pub done: u32,
    pub total: u32,
    pub error: String,
    /// Что фактически отработало — вход для provenance.
    pub note: String,
}

#[derive(Debug)]
struct JobEntry {
    meeting_id: String,
    state: RebuildState,
    done: u32,
    total: u32,
    error: String,
    note: String,
    cancelled: Arc<AtomicBool>,
}

/// Куда отдавать работу. Реальный запуск — поток; тесты подставляют
/// синхронный вариант.
pub trait Spawner: Send + Sync {
    fn spawn(&self, work: Box<dyn FnOnce() + Send + 'static>);
}

/// Рабочий поток на задачу.
pub struct ThreadSpawner;

impl Spawner for ThreadSpawner {
    fn spawn(&self, work: Box<dyn FnOnce() + Send + 'static>) {
        std::thread::spawn(work);
    }
}

/// Выполняет работу на месте — для тестов.
pub struct InlineSpawner;

impl Spawner for InlineSpawner {
    fn spawn(&self, work: Box<dyn FnOnce() + Send + 'static>) {
        work();
    }
}

/// Ручка, которую получает сама работа.
pub struct JobHandle {
    job_id: String,
    jobs: Arc<Mutex<HashMap<String, JobEntry>>>,
    cancelled: Arc<AtomicBool>,
    recording: Arc<AtomicBool>,
}

impl JobHandle {
    /// Сообщить о продвижении. Значения только растут: скачок назад в UI
    /// читается как сбой.
    pub fn report(&self, done: u32, total: u32) {
        let mut guard = self.jobs.lock().expect("jobs poisoned");
        if let Some(entry) = guard.get_mut(&self.job_id) {
            entry.done = entry.done.max(done).min(total);
            entry.total = total;
        }
    }

    /// Записать, что реально отработало: UI обязан называть источник
    /// честно, а не подставлять ожидаемый.
    pub fn set_note(&self, note: impl Into<String>) {
        let mut guard = self.jobs.lock().expect("jobs poisoned");
        if let Some(entry) = guard.get_mut(&self.job_id) {
            entry.note = note.into();
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    /// Идёт ли сейчас запись встречи.
    ///
    /// Пересбор обязан уступать: живое распознавание имеет бюджет
    /// латентности, фоновая работа — нет. Ждать решает сама работа, чтобы
    /// реестр не занимался сном.
    pub fn should_yield(&self) -> bool {
        self.recording.load(Ordering::Relaxed)
    }
}

/// Реестр пересборов.
pub struct RebuildJobs {
    jobs: Arc<Mutex<HashMap<String, JobEntry>>>,
    recording: Arc<AtomicBool>,
    spawner: Box<dyn Spawner>,
}

impl RebuildJobs {
    pub fn new(spawner: Box<dyn Spawner>) -> Self {
        Self {
            jobs: Arc::new(Mutex::new(HashMap::new())),
            recording: Arc::new(AtomicBool::new(false)),
            spawner,
        }
    }

    /// Отметить, что идёт запись; фоновые проходы уступают ей CPU.
    pub fn set_recording(&self, active: bool) {
        self.recording.store(active, Ordering::Relaxed);
    }

    /// Запустить пересбор. Если для встречи уже есть незавершённая
    /// задача — вернуть её id, а не плодить вторую поверх тех же данных.
    pub fn start<F>(&self, job_id: String, meeting_id: String, work: F) -> String
    where
        F: FnOnce(&JobHandle) -> Result<(), String> + Send + 'static,
    {
        if let Some(existing) = self.active_job_for(&meeting_id) {
            return existing;
        }

        let cancelled = Arc::new(AtomicBool::new(false));
        {
            let mut guard = self.jobs.lock().expect("jobs poisoned");
            guard.insert(
                job_id.clone(),
                JobEntry {
                    meeting_id: meeting_id.clone(),
                    state: RebuildState::Queued,
                    done: 0,
                    total: 0,
                    error: String::new(),
                    note: String::new(),
                    cancelled: Arc::clone(&cancelled),
                },
            );
        }

        let handle = JobHandle {
            job_id: job_id.clone(),
            jobs: Arc::clone(&self.jobs),
            cancelled: Arc::clone(&cancelled),
            recording: Arc::clone(&self.recording),
        };
        let jobs = Arc::clone(&self.jobs);
        let id = job_id.clone();

        self.spawner.spawn(Box::new(move || {
            set_state(&jobs, &id, RebuildState::Running, String::new());
            let result = work(&handle);
            // Отмена перекрывает исход работы: прерванный проход не
            // «упал», а был остановлен, и UI должен сказать именно это.
            let (state, error) = if handle.is_cancelled() {
                (RebuildState::Cancelled, String::new())
            } else {
                match result {
                    Ok(()) => (RebuildState::Succeeded, String::new()),
                    Err(message) => (RebuildState::Failed, message),
                }
            };
            set_state(&jobs, &id, state, error);
        }));

        job_id
    }

    pub fn progress(&self, job_id: &str) -> Option<RebuildProgress> {
        let guard = self.jobs.lock().expect("jobs poisoned");
        guard.get(job_id).map(|entry| RebuildProgress {
            job_id: job_id.to_owned(),
            meeting_id: entry.meeting_id.clone(),
            state: entry.state,
            done: entry.done,
            total: entry.total,
            error: entry.error.clone(),
            note: entry.note.clone(),
        })
    }

    /// Попросить остановиться. Работа сама увидит флаг между единицами.
    pub fn cancel(&self, job_id: &str) {
        let guard = self.jobs.lock().expect("jobs poisoned");
        if let Some(entry) = guard.get(job_id) {
            entry.cancelled.store(true, Ordering::Relaxed);
        }
    }

    /// Незавершённая задача этой встречи, если есть.
    pub fn active_job_for(&self, meeting_id: &str) -> Option<String> {
        let guard = self.jobs.lock().expect("jobs poisoned");
        guard
            .iter()
            .find(|(_, entry)| entry.meeting_id == meeting_id && !entry.state.is_terminal())
            .map(|(id, _)| id.clone())
    }

    /// Выбросить завершённые задачи; идущие не трогаются.
    pub fn prune_finished(&self) {
        let mut guard = self.jobs.lock().expect("jobs poisoned");
        guard.retain(|_, entry| !entry.state.is_terminal());
    }
}

fn set_state(
    jobs: &Arc<Mutex<HashMap<String, JobEntry>>>,
    job_id: &str,
    state: RebuildState,
    error: String,
) {
    let mut guard = jobs.lock().expect("jobs poisoned");
    if let Some(entry) = guard.get_mut(job_id) {
        entry.state = state;
        entry.error = error;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry() -> RebuildJobs {
        RebuildJobs::new(Box::new(InlineSpawner))
    }

    #[test]
    fn successful_pass_reports_progress_and_terminal_state() {
        let jobs = registry();

        jobs.start("j1".into(), "m1".into(), |handle| {
            handle.report(1, 2);
            handle.report(2, 2);
            Ok(())
        });

        let progress = jobs.progress("j1").expect("задача должна быть видна");
        assert_eq!(progress.state, RebuildState::Succeeded);
        assert_eq!((progress.done, progress.total), (2, 2));
        assert!(progress.error.is_empty());
        assert_eq!(progress.meeting_id, "m1");
    }

    #[test]
    fn failure_carries_the_reason() {
        let jobs = registry();

        jobs.start(
            "j1".into(),
            "m1".into(),
            |_| Err("модель не скачана".into()),
        );

        let progress = jobs.progress("j1").unwrap();
        assert_eq!(progress.state, RebuildState::Failed);
        assert_eq!(progress.error, "модель не скачана");
    }

    /// Отмена — не сбой: прерванный проход не должен выглядеть упавшим.
    #[test]
    fn cancelled_pass_is_not_a_failure() {
        let jobs = registry();
        let cancelled_seen = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancelled_seen);

        jobs.start("j1".into(), "m1".into(), move |handle| {
            // Реестр не может отменить задачу до её старта при синхронном
            // запуске, поэтому работа отменяет себя сама — проверяем
            // именно трактовку исхода.
            handle.cancelled.store(true, Ordering::Relaxed);
            flag.store(handle.is_cancelled(), Ordering::Relaxed);
            Err("прервано".into())
        });

        let progress = jobs.progress("j1").unwrap();
        assert!(cancelled_seen.load(Ordering::Relaxed));
        assert_eq!(progress.state, RebuildState::Cancelled);
        assert!(progress.error.is_empty(), "отмена не оставляет ошибку");
    }

    #[test]
    fn cancel_sets_the_flag_visible_to_work() {
        let jobs = registry();
        let observed: Arc<Mutex<Vec<bool>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&observed);

        jobs.start("j1".into(), "m1".into(), move |handle| {
            sink.lock().unwrap().push(handle.is_cancelled());
            Ok(())
        });
        jobs.cancel("j1");

        assert_eq!(*observed.lock().unwrap(), vec![false]);
    }

    /// Повторный старт по той же встрече не плодит вторую задачу.
    #[test]
    fn restart_while_active_returns_the_running_job() {
        let jobs = registry();
        // Незавершённая задача: вручную ставим Running без работы.
        jobs.start("j1".into(), "m1".into(), |_| Ok(()));
        set_state(&jobs.jobs, "j1", RebuildState::Running, String::new());

        let second = jobs.start("j2".into(), "m1".into(), |_| Ok(()));

        assert_eq!(second, "j1");
        assert!(jobs.progress("j2").is_none(), "вторая задача не создана");
    }

    #[test]
    fn finished_job_does_not_block_a_new_one() {
        let jobs = registry();
        jobs.start("j1".into(), "m1".into(), |_| Ok(()));

        let second = jobs.start("j2".into(), "m1".into(), |_| Ok(()));

        assert_eq!(second, "j2");
    }

    #[test]
    fn other_meetings_are_independent() {
        let jobs = registry();
        jobs.start("j1".into(), "m1".into(), |_| Ok(()));
        set_state(&jobs.jobs, "j1", RebuildState::Running, String::new());

        let second = jobs.start("j2".into(), "m2".into(), |_| Ok(()));

        assert_eq!(second, "j2");
    }

    /// Прогресс не должен прыгать назад — в UI это читается как сбой.
    #[test]
    fn progress_never_goes_backwards_and_is_clamped() {
        let jobs = registry();

        jobs.start("j1".into(), "m1".into(), |handle| {
            handle.report(5, 10);
            handle.report(2, 10);
            handle.report(99, 10);
            Ok(())
        });

        let progress = jobs.progress("j1").unwrap();
        assert_eq!((progress.done, progress.total), (10, 10));
    }

    #[test]
    fn recording_flag_is_visible_to_work() {
        let jobs = registry();
        jobs.set_recording(true);
        let seen = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&seen);

        jobs.start("j1".into(), "m1".into(), move |handle| {
            flag.store(handle.should_yield(), Ordering::Relaxed);
            Ok(())
        });

        assert!(
            seen.load(Ordering::Relaxed),
            "проход обязан уступать записи"
        );
    }

    #[test]
    fn prune_keeps_running_jobs() {
        let jobs = registry();
        jobs.start("done".into(), "m1".into(), |_| Ok(()));
        jobs.start("live".into(), "m2".into(), |_| Ok(()));
        set_state(&jobs.jobs, "live", RebuildState::Running, String::new());

        jobs.prune_finished();

        assert!(jobs.progress("done").is_none());
        assert!(jobs.progress("live").is_some());
    }

    #[test]
    fn note_carries_what_actually_ran() {
        let jobs = registry();

        jobs.start("j1".into(), "m1".into(), |handle| {
            handle.set_note("re-ASR large-v3");
            Ok(())
        });

        assert_eq!(jobs.progress("j1").unwrap().note, "re-ASR large-v3");
    }

    #[test]
    fn unknown_job_has_no_progress() {
        assert!(registry().progress("nope").is_none());
    }

    #[test]
    fn thread_spawner_runs_the_work() {
        let jobs = RebuildJobs::new(Box::new(ThreadSpawner));
        jobs.start("j1".into(), "m1".into(), |handle| {
            handle.report(1, 1);
            Ok(())
        });

        // Единственное место, где приходится ждать: проверяем сам поток.
        for _ in 0..200 {
            if jobs
                .progress("j1")
                .is_some_and(|progress| progress.state.is_terminal())
            {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("поток не завершил задачу за 2 с");
    }
}
