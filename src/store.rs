//! Local history: tasks and finished sessions, in SQLite.
//!
//! Rows use UUIDs, `updated_at` stamps and tombstones (`deleted`) instead of
//! hard deletes so the same database can later be merged between machines.

use std::{collections::HashMap, path::Path};

use anyhow::Result;
use chrono::{DateTime, Duration as Days, Local, NaiveDate};
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
    /// What the user meant to do in this session, if they wrote it down.
    pub intention: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SessionItem {
    pub id: String,
    #[allow(dead_code)]
    pub phase: Phase,
    pub started_at: i64,
    #[allow(dead_code)]
    pub ended_at: i64,
    pub seconds: u64,
    pub completed: bool,
    #[allow(dead_code)]
    pub task_id: Option<String>,
    pub task_title: Option<String>,
    pub intention: Option<String>,
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
    pub completed_focus: u32,
    pub skipped_focus: u32,
    pub prev_week_focus_seconds: u64,
}

impl Stats {
    pub fn today(&self) -> DayStat {
        self.days.last().map(|(_, s)| *s).unwrap_or_default()
    }

    pub fn last(&self, n: usize) -> &[(NaiveDate, DayStat)] {
        &self.days[self.days.len().saturating_sub(n)..]
    }

    /// Completion rate in whole percent (`0..=100`), or `None` if no focus sessions exist.
    pub fn completion_rate(&self) -> Option<u32> {
        let total = self.completed_focus + self.skipped_focus;
        (total > 0).then(|| ((self.completed_focus as f64 * 100.0) / total as f64).round() as u32)
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

    #[allow(dead_code)]
    pub fn add_task(&self, title: &str) -> Result<Task> {
        self.add_task_with_estimate(title, 1)
    }

    /// Adds a task at the end of the list; `estimate` is clamped to 1..=99.
    pub fn add_task_with_estimate(&self, title: &str, estimate: u32) -> Result<Task> {
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
            estimate: estimate.clamp(1, 99),
            pomodoros: 0,
        };
        self.conn.execute(
            "INSERT INTO tasks (id, title, estimate, position, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
            params![task.id, task.title, task.estimate, position, now],
        )?;
        Ok(task)
    }

    /// Renames a task; a blank title is ignored.
    pub fn rename_task(&self, id: &str, title: &str) -> Result<()> {
        let title = title.trim();
        if title.is_empty() {
            return Ok(());
        }
        self.conn.execute(
            "UPDATE tasks SET title = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, title, now()],
        )?;
        Ok(())
    }

    /// Swaps the task with its neighbour above (`delta < 0`) or below
    /// (`delta > 0`) among the live tasks in the same open/done group.
    pub fn move_task(&self, id: &str, delta: i32) -> Result<()> {
        if delta == 0 {
            return Ok(());
        }
        let tx = self.conn.unchecked_transaction()?;
        let Some((done, position)) = tx
            .query_row(
                "SELECT done, position FROM tasks WHERE id = ?1 AND deleted = 0",
                [id],
                |r| Ok((r.get::<_, bool>(0)?, r.get::<_, f64>(1)?)),
            )
            .optional()?
        else {
            return Ok(());
        };
        let neighbour_sql = if delta < 0 {
            "SELECT id, position FROM tasks
             WHERE deleted = 0 AND done = ?1 AND position < ?2 AND id != ?3
             ORDER BY position DESC LIMIT 1"
        } else {
            "SELECT id, position FROM tasks
             WHERE deleted = 0 AND done = ?1 AND position > ?2 AND id != ?3
             ORDER BY position ASC LIMIT 1"
        };
        let Some((other_id, other_position)) = tx
            .query_row(neighbour_sql, params![done, position, id], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?))
            })
            .optional()?
        else {
            return Ok(());
        };
        let now = now();
        let update = "UPDATE tasks SET position = ?2, updated_at = ?3 WHERE id = ?1";
        tx.execute(update, params![id, other_position, now])?;
        tx.execute(update, params![other_id, position, now])?;
        tx.commit()?;
        Ok(())
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
               (id, phase, started_at, ended_at, seconds, completed, task_id, intention,
                updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?4)",
            params![
                uuid::Uuid::new_v4().to_string(),
                s.phase.key(),
                s.started_at,
                s.ended_at,
                s.seconds as i64,
                s.completed,
                s.task_id,
                s.intention,
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

    pub fn recent_sessions(&self, limit: usize) -> Result<Vec<SessionItem>> {
        let mut stmt = self.conn.prepare(
            "SELECT s.id, s.phase, s.started_at, s.ended_at, s.seconds, s.completed,
                    s.task_id, t.title, s.intention
             FROM sessions s
             LEFT JOIN tasks t ON t.id = s.task_id
             WHERE s.phase = 'focus' AND s.deleted = 0
             ORDER BY s.started_at DESC, s.ended_at DESC
             LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit as i64], |r| {
            let phase: String = r.get(1)?;
            let secs: i64 = r.get(4)?;
            Ok(SessionItem {
                id: r.get(0)?,
                phase: phase_from_key(&phase),
                started_at: r.get(2)?,
                ended_at: r.get(3)?,
                seconds: secs.max(0) as u64,
                completed: r.get(5)?,
                task_id: r.get(6)?,
                task_title: r.get(7)?,
                intention: r.get(8)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn delete_session(&self, id: &str) -> Result<()> {
        let ts = now();
        let tx = self.conn.unchecked_transaction()?;
        let row: Option<(String, bool, Option<String>)> = tx
            .query_row(
                "SELECT phase, completed, task_id FROM sessions WHERE id = ?1 AND deleted = 0",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        if let Some((phase, completed, task_id)) = row {
            tx.execute(
                "UPDATE sessions SET deleted = 1, updated_at = ?2 WHERE id = ?1",
                params![id, ts],
            )?;
            if phase == Phase::Focus.key() && completed {
                if let Some(task_id) = task_id {
                    tx.execute(
                        "UPDATE tasks SET pomodoros = MAX(pomodoros - 1, 0), updated_at = ?2 WHERE id = ?1",
                        params![task_id, ts],
                    )?;
                }
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn add_manual_session(&self, minutes: u32, task_id: Option<String>) -> Result<()> {
        let ended_at = now();
        let seconds = u64::from(minutes.max(1)) * 60;
        let started_at = ended_at - seconds as i64;
        self.record_session(&SessionRecord {
            phase: Phase::Focus,
            started_at,
            ended_at,
            seconds,
            completed: true,
            task_id,
            intention: None,
        })
    }

    pub fn export_sessions_csv(&self) -> Result<String> {
        let mut stmt = self.conn.prepare(
            "SELECT s.started_at, s.ended_at, s.phase, s.seconds, s.completed,
                    t.title, s.intention
             FROM sessions s
             LEFT JOIN tasks t ON t.id = s.task_id
             WHERE s.deleted = 0
             ORDER BY s.started_at ASC, s.ended_at ASC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, bool>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, Option<String>>(6)?,
            ))
        })?;

        let mut out = String::from("started_at,ended_at,phase,minutes,completed,task,intention\r\n");
        for row in rows {
            let (started_at, ended_at, phase, seconds, completed, task, intention) = row?;
            let fields = [
                csv_escape(&format_csv_timestamp(started_at)),
                csv_escape(&format_csv_timestamp(ended_at)),
                csv_escape(&phase),
                format_csv_minutes(seconds.max(0) as u64),
                completed.to_string(),
                csv_escape(task.as_deref().unwrap_or("")),
                csv_escape(intention.as_deref().unwrap_or("")),
            ];
            out.push_str(&fields.join(","));
            out.push_str("\r\n");
        }
        Ok(out)
    }

    /// Focus statistics for the last `days` days (including today).
    pub fn stats(&self, days: u32) -> Result<Stats> {
        let today = Local::now().date_naive();
        self.stats_until(today, days)
    }

    fn stats_until(&self, today: NaiveDate, days: u32) -> Result<Stats> {
        let mut per_day: HashMap<NaiveDate, DayStat> = HashMap::new();
        let mut completed_focus = 0u32;
        let mut skipped_focus = 0u32;
        let mut stmt = self.conn.prepare(
            "SELECT date(started_at, 'unixepoch', 'localtime'), SUM(seconds),
                    SUM(CASE WHEN completed THEN 1 ELSE 0 END),
                    SUM(CASE WHEN completed THEN 0 ELSE 1 END)
             FROM sessions WHERE phase = 'focus' AND deleted = 0
             GROUP BY 1",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
            ))
        })?;
        for row in rows {
            let (date, secs, count, skipped) = row?;
            let count = count.max(0) as u32;
            let skipped = skipped.max(0) as u32;
            completed_focus = completed_focus.saturating_add(count);
            skipped_focus = skipped_focus.saturating_add(skipped);
            if let Ok(date) = NaiveDate::parse_from_str(&date, "%Y-%m-%d") {
                per_day.insert(
                    date,
                    DayStat {
                        focus_seconds: secs.max(0) as u64,
                        pomodoros: count,
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

        let prev_week_focus_seconds: u64 = (7..14)
            .map(|back| {
                let date = today - Days::days(back);
                per_day.get(&date).map(|s| s.focus_seconds).unwrap_or(0)
            })
            .sum();

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
            completed_focus,
            skipped_focus,
            prev_week_focus_seconds,
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

fn phase_from_key(key: &str) -> Phase {
    match key {
        "short_break" => Phase::ShortBreak,
        "long_break" => Phase::LongBreak,
        _ => Phase::Focus,
    }
}

fn csv_escape(field: &str) -> String {
    if field.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_string()
    }
}

fn format_csv_timestamp(ts: i64) -> String {
    DateTime::from_timestamp(ts, 0)
        .map(|dt| dt.with_timezone(&Local).to_rfc3339())
        .unwrap_or_else(|| ts.to_string())
}

fn format_csv_minutes(seconds: u64) -> String {
    match (seconds / 60, seconds % 60) {
        (m, 0) => m.to_string(),
        _ => format!("{:.1}", seconds as f64 / 60.0),
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
                intention: None,
            })
            .unwrap();
    }

    fn titles(store: &Store) -> Vec<String> {
        store.tasks().unwrap().into_iter().map(|t| t.title).collect()
    }

    fn updated_at(store: &Store, id: &str) -> i64 {
        store
            .conn
            .query_row("SELECT updated_at FROM tasks WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .unwrap()
    }

    fn backdate(store: &Store, id: &str) {
        store
            .conn
            .execute("UPDATE tasks SET updated_at = 0 WHERE id = ?1", [id])
            .unwrap();
    }

    #[test]
    fn add_task_with_estimate_clamps() {
        let store = Store::open_in_memory().unwrap();
        let a = store.add_task_with_estimate(" Revisar PR ", 3).unwrap();
        let b = store.add_task_with_estimate("Zero", 0).unwrap();
        let c = store.add_task_with_estimate("Muito", 500).unwrap();
        let d = store.add_task("Padrão").unwrap();
        assert_eq!(a.title, "Revisar PR");
        assert_eq!(a.estimate, 3);
        assert_eq!(b.estimate, 1);
        assert_eq!(c.estimate, 99);
        assert_eq!(d.estimate, 1);
        let stored: Vec<u32> = store.tasks().unwrap().iter().map(|t| t.estimate).collect();
        assert_eq!(stored, vec![3, 1, 99, 1]);
        assert_eq!(store.task(&a.id).unwrap().unwrap(), a);
    }

    #[test]
    fn rename_task_trims_and_ignores_blank() {
        let store = Store::open_in_memory().unwrap();
        let a = store.add_task("Rascunho").unwrap();
        backdate(&store, &a.id);
        store.rename_task(&a.id, "   ").unwrap();
        assert_eq!(store.task(&a.id).unwrap().unwrap().title, "Rascunho");
        assert_eq!(updated_at(&store, &a.id), 0, "blank rename is a no-op");
        store.rename_task(&a.id, "  Versão final ").unwrap();
        assert_eq!(store.task(&a.id).unwrap().unwrap().title, "Versão final");
        assert!(updated_at(&store, &a.id) > 0);
        // Unknown ids are harmless.
        store.rename_task("missing", "Nada").unwrap();
    }

    #[test]
    fn move_task_swaps_with_neighbour() {
        let store = Store::open_in_memory().unwrap();
        let a = store.add_task("A").unwrap();
        let b = store.add_task("B").unwrap();
        let c = store.add_task("C").unwrap();
        let d = store.add_task("D").unwrap();
        for t in [&a, &b, &c, &d] {
            backdate(&store, &t.id);
        }

        store.move_task(&c.id, -1).unwrap();
        assert_eq!(titles(&store), ["A", "C", "B", "D"]);
        assert!(updated_at(&store, &c.id) > 0);
        assert!(updated_at(&store, &b.id) > 0);
        assert_eq!(updated_at(&store, &a.id), 0, "others untouched");

        store.move_task(&a.id, 1).unwrap();
        assert_eq!(titles(&store), ["C", "A", "B", "D"]);

        // Edges and no-ops.
        backdate(&store, &c.id);
        store.move_task(&c.id, -1).unwrap();
        store.move_task(&d.id, 1).unwrap();
        store.move_task(&b.id, 0).unwrap();
        store.move_task("missing", 1).unwrap();
        assert_eq!(titles(&store), ["C", "A", "B", "D"]);
        assert_eq!(updated_at(&store, &c.id), 0, "moving past the top is a no-op");

        // Done and deleted tasks are skipped: B's neighbour below is now nothing,
        // and above it is C (A is deleted).
        store.set_task_done(&d.id, true).unwrap();
        store.delete_task(&a.id).unwrap();
        store.move_task(&b.id, 1).unwrap();
        assert_eq!(titles(&store), ["C", "B", "D"]);
        store.move_task(&b.id, -1).unwrap();
        assert_eq!(titles(&store), ["B", "C", "D"]);

        // Done tasks reorder among themselves only.
        let e = store.add_task("E").unwrap();
        store.set_task_done(&e.id, true).unwrap();
        store.move_task(&e.id, -1).unwrap();
        assert_eq!(titles(&store), ["B", "C", "E", "D"]);
        store.move_task(&e.id, -1).unwrap();
        assert_eq!(titles(&store), ["B", "C", "E", "D"]);
    }

    #[test]
    fn record_session_stores_intention() {
        let store = Store::open_in_memory().unwrap();
        let session = |intention: Option<&str>| SessionRecord {
            phase: Phase::Focus,
            started_at: 1_000,
            ended_at: 2_500,
            seconds: 1500,
            completed: true,
            task_id: None,
            intention: intention.map(Into::into),
        };
        store
            .record_session(&session(Some("Fechar o capítulo 2")))
            .unwrap();
        store.record_session(&session(None)).unwrap();
        let mut stmt = store
            .conn
            .prepare("SELECT intention FROM sessions ORDER BY intention IS NULL")
            .unwrap();
        let stored: Vec<Option<String>> = stmt
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(stored, vec![Some("Fechar o capítulo 2".to_string()), None]);
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
        // Gap, then an older 3-day run (days 5, 6, 7 back; day 7 falls in prev week).
        for back in 5..8 {
            focus_at(&store, today - Days::days(back), true, None);
        }
        // Days 9, 10, 11, 12 back all fall in prev week (7..14).
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
        assert_eq!(stats.completed_focus, 10);
        assert_eq!(stats.skipped_focus, 1);
        assert_eq!(stats.completion_rate(), Some(91));
        // Prev week covers days 7..14 back: days 7, 9, 10, 11, 12 = 5 sessions * 1500s.
        assert_eq!(stats.prev_week_focus_seconds, 5 * 1500);
        assert_eq!(store.task(&task.id).unwrap().unwrap().pomodoros, 1);

        // A day without focus yet keeps yesterday's streak alive.
        let tomorrow = store.stats_until(today + Days::days(1), 7).unwrap();
        assert_eq!(tomorrow.streak, 3);
        let later = store.stats_until(today + Days::days(2), 7).unwrap();
        assert_eq!(later.streak, 0);
    }

    #[test]
    fn recent_sessions_and_delete_decrements_pomodoros() {
        let store = Store::open_in_memory().unwrap();
        let task = store.add_task("Refatorar store").unwrap();

        // Older completed focus linked to task.
        store
            .record_session(&SessionRecord {
                phase: Phase::Focus,
                started_at: 1_700_000_000,
                ended_at: 1_700_001_500,
                seconds: 1500,
                completed: true,
                task_id: Some(task.id.clone()),
                intention: None,
            })
            .unwrap();
        // A break (should NOT appear in recent_sessions).
        store
            .record_session(&SessionRecord {
                phase: Phase::ShortBreak,
                started_at: 1_700_001_600,
                ended_at: 1_700_001_900,
                seconds: 300,
                completed: true,
                task_id: None,
                intention: None,
            })
            .unwrap();
        // Newer skipped focus linked to task (does not increment pomodoros).
        store
            .record_session(&SessionRecord {
                phase: Phase::Focus,
                started_at: 1_700_002_000,
                ended_at: 1_700_002_600,
                seconds: 600,
                completed: false,
                task_id: Some(task.id.clone()),
                intention: None,
            })
            .unwrap();

        assert_eq!(store.task(&task.id).unwrap().unwrap().pomodoros, 1);

        let limited = store.recent_sessions(1).unwrap();
        assert_eq!(limited.len(), 1);
        assert_eq!(limited[0].started_at, 1_700_002_000);

        let recent = store.recent_sessions(10).unwrap();
        assert_eq!(recent.len(), 2);
        assert!(!recent[0].completed);
        assert_eq!(recent[0].task_title.as_deref(), Some("Refatorar store"));
        assert!(recent[1].completed);
        assert_eq!(recent[1].task_id.as_deref(), Some(task.id.as_str()));

        // Deleting a skipped session must NOT decrement task.pomodoros.
        store.delete_session(&recent[0].id).unwrap();
        assert_eq!(store.task(&task.id).unwrap().unwrap().pomodoros, 1);
        assert_eq!(store.recent_sessions(10).unwrap().len(), 1);

        // Deleting the completed focus session decrements task.pomodoros to 0.
        store.delete_session(&recent[1].id).unwrap();
        assert_eq!(store.task(&task.id).unwrap().unwrap().pomodoros, 0);
        assert!(store.recent_sessions(10).unwrap().is_empty());

        // Calling delete_session again is idempotent and never underflows.
        store.delete_session(&recent[1].id).unwrap();
        assert_eq!(store.task(&task.id).unwrap().unwrap().pomodoros, 0);
    }

    #[test]
    fn add_manual_session_records_and_increments() {
        let store = Store::open_in_memory().unwrap();
        let task = store.add_task("Estudar Rust").unwrap();

        store
            .add_manual_session(30, Some(task.id.clone()))
            .unwrap();
        store.add_manual_session(15, None).unwrap();

        assert_eq!(store.task(&task.id).unwrap().unwrap().pomodoros, 1);
        let recent = store.recent_sessions(10).unwrap();
        assert_eq!(recent.len(), 2);
        for item in &recent {
            assert_eq!(item.phase, Phase::Focus);
            assert!(item.completed);
            assert_eq!(item.ended_at - item.started_at, item.seconds as i64);
        }
        let secs: Vec<u64> = recent.iter().map(|s| s.seconds).collect();
        assert!(secs.contains(&1800));
        assert!(secs.contains(&900));

        let stats = store.stats(7).unwrap();
        assert_eq!(stats.today().pomodoros, 2);
        assert_eq!(stats.today().focus_seconds, 2700);
        assert_eq!(stats.completed_focus, 2);
        assert_eq!(stats.skipped_focus, 0);
    }

    #[test]
    fn export_sessions_csv_rfc4180() {
        let store = Store::open_in_memory().unwrap();
        let task = store.add_task("Revisar \"PR\", docs").unwrap();

        // Insert out of chronological order + include a break, an intention with newline, and a deleted row.
        store
            .conn
            .execute(
                "INSERT INTO sessions
                   (id, phase, started_at, ended_at, seconds, completed, task_id, intention, updated_at, deleted)
                 VALUES (?1, 'short_break', 1700002000, 1700002090, 90, 1, NULL, ?2, 1700002090, 0)",
                params!["s-break", "Alongar\nrespirar"],
            )
            .unwrap();
        store
            .conn
            .execute(
                "INSERT INTO sessions
                   (id, phase, started_at, ended_at, seconds, completed, task_id, intention, updated_at, deleted)
                 VALUES (?1, 'focus', 1700000000, 1700001500, 1500, 1, ?2, NULL, 1700001500, 0)",
                params!["s-focus", task.id],
            )
            .unwrap();
        store
            .conn
            .execute(
                "INSERT INTO sessions
                   (id, phase, started_at, ended_at, seconds, completed, task_id, intention, updated_at, deleted)
                 VALUES (?1, 'focus', 1700003000, 1700004500, 1500, 0, NULL, 'Ignorada', 1700004500, 1)",
                ["s-deleted"],
            )
            .unwrap();

        let csv = store.export_sessions_csv().unwrap();
        let expected_focus_start = format_csv_timestamp(1_700_000_000);
        let expected_focus_end = format_csv_timestamp(1_700_001_500);
        let expected_break_start = format_csv_timestamp(1_700_002_000);
        let expected_break_end = format_csv_timestamp(1_700_002_090);

        let expected = format!(
            "started_at,ended_at,phase,minutes,completed,task,intention\r\n\
             {expected_focus_start},{expected_focus_end},focus,25,true,\"Revisar \"\"PR\"\", docs\",\r\n\
             {expected_break_start},{expected_break_end},short_break,1.5,true,,\"Alongar\nrespirar\"\r\n"
        );
        assert_eq!(csv, expected);
    }
}
