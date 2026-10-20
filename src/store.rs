//! Local history: tasks and finished sessions, in SQLite.
//!
//! Rows use UUIDs, `updated_at` stamps and tombstones (`deleted`) instead of
//! hard deletes so the same database can later be merged between machines.

use std::{collections::HashMap, path::Path};

use anyhow::Result;
use chrono::{Duration as Days, Local, NaiveDate};
use rusqlite::{params, Connection, OptionalExtension};

use crate::timer::Phase;

#[derive(Clone, Debug, PartialEq)]
pub struct Task {
    pub id: String,
    pub title: String,
    pub done: bool,
    pub estimate: u32,
    pub pomodoros: u32,
}

#[derive(Clone, Debug)]
pub struct SessionRecord {
    pub phase: Phase,
    pub started_at: i64,
    pub ended_at: i64,
    pub seconds: u64,
    pub completed: bool,
    pub task_id: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DayStat {
    pub focus_seconds: u64,
    pub pomodoros: u32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stats {
    /// Oldest first, one entry per day, ending today.
    pub days: Vec<(NaiveDate, DayStat)>,
    pub streak: u32,
    pub best_streak: u32,
    pub total_pomodoros: u32,
    pub total_focus_seconds: u64,
}

impl Stats {
    pub fn today(&self) -> DayStat {
        self.days.last().map(|(_, s)| *s).unwrap_or_default()
    }

    pub fn last(&self, n: usize) -> &[(NaiveDate, DayStat)] {
        &self.days[self.days.len().saturating_sub(n)..]
    }
}

pub struct Store {
    conn: Connection,
}

const SCHEMA_VERSION: i32 = 1;

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        Self::init(Connection::open(path)?)
    }

    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        let version: i32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version < 1 {
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS tasks (
                    id TEXT PRIMARY KEY,
                    title TEXT NOT NULL,
                    done INTEGER NOT NULL DEFAULT 0,
                    estimate INTEGER NOT NULL DEFAULT 1,
                    pomodoros INTEGER NOT NULL DEFAULT 0,
                    position REAL NOT NULL,
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL,
                    done_at INTEGER,
                    deleted INTEGER NOT NULL DEFAULT 0
                );
                CREATE TABLE IF NOT EXISTS sessions (
                    id TEXT PRIMARY KEY,
                    phase TEXT NOT NULL,
                    started_at INTEGER NOT NULL,
                    ended_at INTEGER NOT NULL,
                    seconds INTEGER NOT NULL,
                    completed INTEGER NOT NULL,
                    task_id TEXT,
                    intention TEXT,
                    updated_at INTEGER NOT NULL,
                    deleted INTEGER NOT NULL DEFAULT 0
                );
                CREATE INDEX IF NOT EXISTS sessions_started ON sessions(started_at);",
            )?;
        }
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        Ok(Self { conn })
    }

    pub fn tasks(&self) -> Result<Vec<Task>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, done, estimate, pomodoros FROM tasks
             WHERE deleted = 0 ORDER BY done, position",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Task {
                id: r.get(0)?,
                title: r.get(1)?,
                done: r.get(2)?,
                estimate: r.get(3)?,
                pomodoros: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn add_task(&self, title: &str) -> Result<Task> {
        let now = now();
        let position: f64 = self.conn.query_row(
            "SELECT COALESCE(MAX(position), 0) + 1 FROM tasks",
            [],
            |r| r.get(0),
        )?;
        let task = Task {
            id: uuid::Uuid::new_v4().to_string(),
            title: title.trim().to_string(),
            done: false,
            estimate: 1,
            pomodoros: 0,
        };
        self.conn.execute(
            "INSERT INTO tasks (id, title, position, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?4)",
            params![task.id, task.title, position, now],
        )?;
        Ok(task)
    }

    pub fn set_task_done(&self, id: &str, done: bool) -> Result<()> {
        let now = now();
        self.conn.execute(
            "UPDATE tasks SET done = ?2, done_at = CASE WHEN ?2 THEN ?3 END, updated_at = ?3
             WHERE id = ?1",
            params![id, done, now],
        )?;
        Ok(())
    }

    pub fn adjust_estimate(&self, id: &str, delta: i32) -> Result<()> {
        self.conn.execute(
            "UPDATE tasks SET estimate = MIN(MAX(estimate + ?2, 1), 99), updated_at = ?3
             WHERE id = ?1",
            params![id, delta, now()],
        )?;
        Ok(())
    }

    pub fn delete_task(&self, id: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE tasks SET deleted = 1, updated_at = ?2 WHERE id = ?1",
            params![id, now()],
        )?;
        Ok(())
    }

    pub fn clear_done_tasks(&self) -> Result<()> {
        self.conn.execute(
            "UPDATE tasks SET deleted = 1, updated_at = ?1 WHERE done = 1 AND deleted = 0",
            params![now()],
        )?;
        Ok(())
    }

    pub fn record_session(&self, s: &SessionRecord) -> Result<()> {
        self.conn.execute(
            "INSERT INTO sessions
               (id, phase, started_at, ended_at, seconds, completed, task_id, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?4)",
            params![
                uuid::Uuid::new_v4().to_string(),
                s.phase.key(),
                s.started_at,
                s.ended_at,
                s.seconds as i64,
                s.completed,
                s.task_id,
            ],
        )?;
        if s.phase == Phase::Focus && s.completed {
            if let Some(task_id) = &s.task_id {
                self.conn.execute(
                    "UPDATE tasks SET pomodoros = pomodoros + 1, updated_at = ?2 WHERE id = ?1",
                    params![task_id, s.ended_at],
                )?;
            }
        }
        Ok(())
    }

    /// Focus statistics for the last `days` days (including today).
    pub fn stats(&self, days: u32) -> Result<Stats> {
        let today = Local::now().date_naive();
        self.stats_until(today, days)
    }

    fn stats_until(&self, today: NaiveDate, days: u32) -> Result<Stats> {
        let mut per_day: HashMap<NaiveDate, DayStat> = HashMap::new();
        let mut stmt = self.conn.prepare(
            "SELECT date(started_at, 'unixepoch', 'localtime'), SUM(seconds),
                    SUM(CASE WHEN completed THEN 1 ELSE 0 END)
             FROM sessions WHERE phase = 'focus' AND deleted = 0
             GROUP BY 1",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })?;
        for row in rows {
            let (date, secs, count) = row?;
            if let Ok(date) = NaiveDate::parse_from_str(&date, "%Y-%m-%d") {
                per_day.insert(
                    date,
                    DayStat {
                        focus_seconds: secs.max(0) as u64,
                        pomodoros: count.max(0) as u32,
                    },
                );
            }
        }

        let days_list = (0..days.max(1))
            .rev()
            .map(|back| {
                let date = today - Days::days(back as i64);
                (date, per_day.get(&date).copied().unwrap_or_default())
            })
            .collect();

        let active = |d: &NaiveDate| per_day.get(d).is_some_and(|s| s.pomodoros > 0);

        // Today without a pomodoro yet doesn't break the streak.
        let mut cursor = if active(&today) {
            today
        } else {
            today - Days::days(1)
        };
        let mut streak = 0;
        while active(&cursor) {
            streak += 1;
            cursor -= Days::days(1);
        }

        let mut active_days: Vec<_> = per_day
            .iter()
            .filter(|(_, s)| s.pomodoros > 0)
            .map(|(d, _)| *d)
            .collect();
        active_days.sort();
        let (mut best, mut run, mut prev) = (0u32, 0u32, None::<NaiveDate>);
        for day in active_days {
            run = match prev {
                Some(p) if day - p == Days::days(1) => run + 1,
                _ => 1,
            };
            best = best.max(run);
            prev = Some(day);
        }

        Ok(Stats {
            days: days_list,
            streak,
            best_streak: best,
            total_pomodoros: per_day.values().map(|s| s.pomodoros).sum(),
            total_focus_seconds: per_day.values().map(|s| s.focus_seconds).sum(),
        })
    }

    #[allow(dead_code)]
    pub fn task(&self, id: &str) -> Result<Option<Task>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, title, done, estimate, pomodoros FROM tasks WHERE id = ?1",
                [id],
                |r| {
                    Ok(Task {
                        id: r.get(0)?,
                        title: r.get(1)?,
                        done: r.get(2)?,
                        estimate: r.get(3)?,
                        pomodoros: r.get(4)?,
                    })
                },
            )
            .optional()?)
    }
}

pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn focus_at(store: &Store, date: NaiveDate, completed: bool, task: Option<&str>) {
        let start = Local
            .from_local_datetime(&date.and_hms_opt(10, 0, 0).unwrap())
            .unwrap()
            .timestamp();
        store
            .record_session(&SessionRecord {
                phase: Phase::Focus,
                started_at: start,
                ended_at: start + 1500,
                seconds: 1500,
                completed,
                task_id: task.map(Into::into),
            })
            .unwrap();
    }

    #[test]
    fn tasks_roundtrip() {
        let store = Store::open_in_memory().unwrap();
        let a = store.add_task("  Escrever relatório ").unwrap();
        let b = store.add_task("Revisar PR").unwrap();
        assert_eq!(a.title, "Escrever relatório");
        store.set_task_done(&a.id, true).unwrap();
        store.adjust_estimate(&b.id, 3).unwrap();
        store.adjust_estimate(&b.id, -10).unwrap();
        let tasks = store.tasks().unwrap();
        assert_eq!(tasks[0].id, b.id, "open tasks come first");
        assert_eq!(tasks[0].estimate, 1);
        assert!(tasks[1].done);
        store.clear_done_tasks().unwrap();
        assert_eq!(store.tasks().unwrap().len(), 1);
        store.delete_task(&b.id).unwrap();
        assert!(store.tasks().unwrap().is_empty());
    }

    #[test]
    fn stats_and_streaks() {
        let store = Store::open_in_memory().unwrap();
        let today = NaiveDate::from_ymd_opt(2026, 10, 20).unwrap();
        let task = store.add_task("Tese").unwrap();
        focus_at(&store, today, true, Some(&task.id));
        focus_at(&store, today, false, None);
        focus_at(&store, today - Days::days(1), true, None);
        focus_at(&store, today - Days::days(2), true, None);
        // Gap, then an older 3-day run.
        for back in 5..8 {
            focus_at(&store, today - Days::days(back), true, None);
        }
        focus_at(&store, today - Days::days(9), true, None);
        focus_at(&store, today - Days::days(10), true, None);
        focus_at(&store, today - Days::days(11), true, None);
        focus_at(&store, today - Days::days(12), true, None);

        let stats = store.stats_until(today, 14).unwrap();
        assert_eq!(stats.days.len(), 14);
        assert_eq!(stats.days.last().unwrap().0, today);
        assert_eq!(stats.today().pomodoros, 1);
        assert_eq!(stats.today().focus_seconds, 3000);
        assert_eq!(stats.streak, 3);
        assert_eq!(stats.best_streak, 4);
        assert_eq!(stats.total_pomodoros, 10);
        assert_eq!(store.task(&task.id).unwrap().unwrap().pomodoros, 1);

        // A day without focus yet keeps yesterday's streak alive.
        let tomorrow = store.stats_until(today + Days::days(1), 7).unwrap();
        assert_eq!(tomorrow.streak, 3);
        let later = store.stats_until(today + Days::days(2), 7).unwrap();
        assert_eq!(later.streak, 0);
    }
}
