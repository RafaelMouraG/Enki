//! The application model shared by every window: timer, tasks, stats,
//! settings and the background services (media, tray, wallpaper).

use std::time::{Duration, Instant};

use chrono::{Local, NaiveDate};

use gpui::{
    App, AppContext as _, AsyncApp, Context, Entity, Global, Rgba, Task, WeakEntity,
    WindowAppearance,
};

use crate::{
    media::{MediaCommand, MediaWatcher, NowPlaying},
    notify,
    settings::Settings,
    store::{now, SessionItem, SessionRecord, Stats, Store, Task as TaskItem},
    theme::Theme,
    timer::{format_clock, Finished, Phase, Timer},
    tray::{Tray, TrayAction},
    wallpaper::{self, Wallpaper},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Timer,
    Tasks,
    Stats,
    Settings(SettingsSection),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsSection {
    General,
    Appearance,
    Timer,
    Notifications,
    Shortcuts,
}

impl SettingsSection {
    pub const ALL: [SettingsSection; 5] = [
        SettingsSection::General,
        SettingsSection::Appearance,
        SettingsSection::Timer,
        SettingsSection::Notifications,
        SettingsSection::Shortcuts,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SettingsSection::General => "Geral",
            SettingsSection::Appearance => "Aparência",
            SettingsSection::Timer => "Timer",
            SettingsSection::Notifications => "Notificações",
            SettingsSection::Shortcuts => "Atalhos",
        }
    }
}

/// How many days of history the stats page shows (26 weeks).
pub const STATS_DAYS: u32 = 7 * 26;
/// How many recent focus sessions to show on the stats page.
pub const RECENT_SESSIONS_LIMIT: usize = 20;

const TICK: Duration = Duration::from_millis(250);
const MIN_INCOMPLETE_SECONDS: u64 = 60;

pub struct Enki {
    pub settings: Settings,
    pub timer: Timer,
    store: Option<Store>,
    pub tasks: Vec<TaskItem>,
    pub stats: Stats,
    pub recent_sessions: Vec<SessionItem>,
    /// The local day `stats` was computed for, to refresh "today" after midnight.
    stats_day: NaiveDate,
    pub current_task: Option<String>,
    /// Free-text intention for the focus session, saved with each focus record.
    pub intention: String,
    pub page: Page,
    pub wallpaper: Option<Wallpaper>,
    pub now_playing: Option<NowPlaying>,
    pub system_dark: bool,
    pub error: Option<String>,
    /// Wall-clock start of the phase currently on the clock, for the history.
    phase_started_at: Option<i64>,
    media: MediaWatcher,
    /// True when Enki paused playback entering a break, so it can resume on focus.
    media_paused_by_enki: bool,
    tray: Option<Tray>,
    /// Creating the tray failed; don't retry every tick until the setting changes.
    tray_failed: bool,
    _tick: Task<()>,
    wallpaper_job: Option<Task<()>>,
}

pub struct AppModel(pub Entity<Enki>);

impl Global for AppModel {}

pub fn model(cx: &App) -> Entity<Enki> {
    cx.global::<AppModel>().0.clone()
}

impl Enki {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let settings = Settings::load();
        let (store, error) = match Store::open(&crate::paths::data_dir().join("enki.db")) {
            Ok(store) => (Some(store), None),
            Err(err) => (
                None,
                Some(format!("Não foi possível abrir o histórico: {err:#}")),
            ),
        };
        let media = MediaWatcher::spawn(settings.now_playing);
        let tick = cx.spawn(
            async move |this: WeakEntity<Enki>, cx: &mut AsyncApp| loop {
                cx.background_executor().timer(TICK).await;
                if this.update(cx, |this, cx| this.tick(cx)).is_err() {
                    break;
                }
            },
        );

        let mut enki = Self {
            timer: Timer::new(settings.durations()),
            settings,
            store,
            tasks: Vec::new(),
            stats: Stats::default(),
            recent_sessions: Vec::new(),
            stats_day: Local::now().date_naive(),
            current_task: None,
            intention: String::new(),
            page: Page::Timer,
            wallpaper: None,
            now_playing: None,
            system_dark: is_dark(cx.window_appearance()),
            error,
            phase_started_at: None,
            media,
            media_paused_by_enki: false,
            tray: None,
            tray_failed: false,
            _tick: tick,
            wallpaper_job: None,
        };
        enki.reload_tasks();
        enki.reload_stats();
        #[cfg(debug_assertions)]
        enki.apply_debug_env();
        enki.sync_tray();
        enki.load_wallpaper(cx);
        enki
    }

    // ---- Theme -------------------------------------------------------------

    pub fn build_theme(&self) -> Theme {
        let tint: Option<Rgba> = self.wallpaper.as_ref().map(|w| w.tint);
        Theme::build(&self.settings, self.system_dark, tint)
    }

    pub fn apply_theme(&self, cx: &mut App) {
        cx.set_global(self.build_theme());
        crate::sync_window_backgrounds(cx);
        cx.refresh_windows();
    }

    pub fn set_system_appearance(&mut self, appearance: WindowAppearance, cx: &mut Context<Self>) {
        let dark = is_dark(appearance);
        if dark != self.system_dark {
            self.system_dark = dark;
            self.apply_theme(cx);
            cx.notify();
        }
    }

    // ---- Settings ----------------------------------------------------------

    pub fn update_settings(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut Settings)) {
        let before = self.settings.clone();
        f(&mut self.settings);
        if self.settings == before {
            return;
        }
        if let Err(err) = self.settings.save() {
            self.error = Some(format!("Não foi possível salvar as configurações: {err:#}"));
        }
        let s = self.settings.clone();
        if s.durations() != before.durations() {
            self.timer.set_durations(s.durations());
        }
        if s.now_playing != before.now_playing {
            self.media.set_enabled(s.now_playing);
            if !s.now_playing {
                self.now_playing = None;
                self.media_paused_by_enki = false;
            }
        }
        if !s.auto_pause_media {
            self.media_paused_by_enki = false;
        }
        if s.menu_bar != before.menu_bar {
            self.tray_failed = false;
            self.sync_tray();
        }
        if s.mini_on_top != before.mini_on_top {
            cx.defer(crate::sync_mini_window);
        }
        if s.background_image != before.background_image
            || s.background_effect != before.background_effect
        {
            self.load_wallpaper(cx);
        }
        self.apply_theme(cx);
        cx.notify();
    }

    pub fn apply_preset(
        &mut self,
        focus: u32,
        short_break: u32,
        long_break: u32,
        cx: &mut Context<Self>,
    ) {
        self.update_settings(cx, |s| {
            s.focus_minutes = focus;
            s.short_break_minutes = short_break;
            s.long_break_minutes = long_break;
        });
        self.sync_tray();
    }

    // ---- Wallpaper ---------------------------------------------------------

    fn load_wallpaper(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.settings.background_image.clone() else {
            self.wallpaper = None;
            self.wallpaper_job = None;
            return;
        };
        let effect = self.settings.background_effect;
        // Screenshots of a locked/occluded session only get the first frame.
        if cfg!(debug_assertions) && std::env::var("ENKI_DEBUG_SYNC").is_ok() {
            self.wallpaper = wallpaper::process(&path, effect).ok();
            return;
        }
        let job = cx
            .background_executor()
            .spawn(async move { wallpaper::process(&path, effect) });
        self.wallpaper_job = Some(cx.spawn(async move |this, cx| {
            let result = job.await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(wallpaper) => {
                        this.wallpaper = Some(wallpaper);
                        this.error = None;
                    }
                    Err(err) => {
                        this.wallpaper = None;
                        this.error = Some(format!("Não foi possível carregar a imagem: {err:#}"));
                    }
                }
                this.apply_theme(cx);
                cx.notify();
            })
            .ok();
        }));
    }

    // ---- Timer -------------------------------------------------------------

    pub fn toggle_timer(&mut self, cx: &mut Context<Self>) {
        let now = Instant::now();
        let was_running = self.timer.is_running();
        if self.timer.is_idle() {
            self.phase_started_at = Some(crate::store::now());
        }
        self.timer.toggle(now);
        if !was_running && self.timer.is_running() && self.timer.phase() == Phase::Focus {
            self.resume_media_for_focus(cx);
        }
        self.sync_tray();
        cx.notify();
    }

    pub fn extend_timer(&mut self, extra: Duration, cx: &mut Context<Self>) {
        self.timer.extend(extra);
        self.sync_tray();
        cx.notify();
    }

    pub fn reset_timer(&mut self, cx: &mut Context<Self>) {
        self.timer.reset();
        self.phase_started_at = None;
        self.sync_tray();
        cx.notify();
    }

    pub fn skip_phase(&mut self, cx: &mut Context<Self>) {
        let now = Instant::now();
        if let Some(finished) = self.timer.skip(now) {
            if finished.completed || finished.elapsed.as_secs() >= MIN_INCOMPLETE_SECONDS {
                self.record(finished);
            }
        }
        self.phase_started_at = None;
        self.sync_tray();
        cx.notify();
    }

    pub fn select_phase(&mut self, phase: Phase, cx: &mut Context<Self>) {
        if phase == self.timer.phase() && !self.timer.is_idle() {
            return;
        }
        let now = Instant::now();
        // Switching away from a running phase keeps what was done so far.
        let elapsed = self.timer.elapsed(now);
        if !elapsed.is_zero() {
            let finished = Finished {
                phase: self.timer.phase(),
                elapsed,
                completed: false,
            };
            if finished.completed || finished.elapsed.as_secs() >= MIN_INCOMPLETE_SECONDS {
                self.record(finished);
            }
        }
        self.timer.select_phase(phase);
        self.phase_started_at = None;
        self.sync_tray();
        cx.notify();
    }

    fn tick(&mut self, cx: &mut Context<Self>) {
        let now = Instant::now();
        let settings = &self.settings;
        let (auto_breaks, auto_focus) = (settings.auto_start_breaks, settings.auto_start_focus);
        if let Some(finished) = self.timer.tick(now, |next| {
            if next.is_break() {
                auto_breaks
            } else {
                auto_focus
            }
        }) {
            self.record(finished);
            self.phase_started_at = self.timer.is_running().then(crate::store::now);
            if finished.phase == Phase::Focus && self.timer.phase().is_break() {
                self.pause_media_for_break(cx);
            } else if finished.phase.is_break()
                && self.timer.phase() == Phase::Focus
                && self.timer.is_running()
            {
                self.resume_media_for_focus(cx);
            }
            self.announce(finished);
            cx.notify();
        } else if self.timer.is_running() {
            cx.notify();
        }

        if Local::now().date_naive() != self.stats_day {
            self.reload_stats();
            cx.notify();
        }

        let playing = self.media.current();
        if playing != self.now_playing {
            self.now_playing = playing;
            cx.notify();
        }

        for action in self.tray.as_ref().map(Tray::poll).unwrap_or_default() {
            self.handle_tray(action, cx);
        }
        self.sync_tray();
    }

    fn handle_tray(&mut self, action: TrayAction, cx: &mut Context<Self>) {
        match action {
            TrayAction::Toggle => self.toggle_timer(cx),
            TrayAction::Skip => self.skip_phase(cx),
            TrayAction::Reset => self.reset_timer(cx),
            TrayAction::Show => cx.defer(crate::show_main_window),
            TrayAction::Mini => cx.defer(crate::toggle_mini_window),
            TrayAction::Quit => cx.quit(),
        }
    }

    fn record(&mut self, finished: Finished) {
        let ended_at = now();
        let started_at = self
            .phase_started_at
            .unwrap_or(ended_at - finished.elapsed.as_secs() as i64);
        let record = SessionRecord {
            phase: finished.phase,
            started_at,
            ended_at,
            seconds: finished.elapsed.as_secs(),
            completed: finished.completed,
            task_id: self.current_task.clone(),
            intention: (finished.phase == Phase::Focus)
                .then(|| self.intention.trim())
                .filter(|text| !text.is_empty())
                .map(str::to_string),
        };
        if let Some(store) = &self.store {
            if let Err(err) = store.record_session(&record) {
                self.error = Some(format!("Não foi possível salvar a sessão: {err:#}"));
            }
        }
        self.reload_tasks();
        self.reload_stats();
    }

    fn announce(&self, finished: Finished) {
        let s = &self.settings;
        notify::play(s.sound);
        if !s.notifications {
            return;
        }
        let next = self.timer.phase();
        let mins = |d: Duration| d.as_secs() / 60;
        let durations = self.timer.durations();
        let (title, body) = match finished.phase {
            Phase::Focus => {
                let title = "Foco concluído".to_string();
                let body = if next == Phase::LongBreak {
                    format!(
                        "Ciclo completo! Pausa longa de {} min.",
                        mins(durations.long_break)
                    )
                } else {
                    format!("Hora de uma pausa de {} min.", mins(durations.short_break))
                };
                (title, body)
            }
            _ => {
                let title = "Pausa encerrada".to_string();
                let body = match self.current_task_title() {
                    Some(task) => format!("De volta a: {task}"),
                    None => format!("Pronto para mais {} min de foco?", mins(durations.focus)),
                };
                (title, body)
            }
        };
        notify::notify(&title, &body);
    }

    // ---- Tasks -------------------------------------------------------------

    fn reload_tasks(&mut self) {
        if let Some(store) = &self.store {
            match store.tasks() {
                Ok(tasks) => self.tasks = tasks,
                Err(err) => self.error = Some(format!("Erro ao ler tarefas: {err:#}")),
            }
        }
        if let Some(id) = &self.current_task {
            if !self.tasks.iter().any(|t| &t.id == id && !t.done) {
                self.current_task = None;
            }
        }
    }

    fn reload_stats(&mut self) {
        self.stats_day = Local::now().date_naive();
        if let Some(store) = &self.store {
            match store.stats(STATS_DAYS) {
                Ok(stats) => self.stats = stats,
                Err(err) => self.error = Some(format!("Erro ao ler estatísticas: {err:#}")),
            }
            match store.recent_sessions(RECENT_SESSIONS_LIMIT) {
                Ok(sessions) => self.recent_sessions = sessions,
                Err(err) => self.error = Some(format!("Erro ao ler sessões recentes: {err:#}")),
            }
        }
    }

    pub fn delete_session(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(store) = &self.store {
            if let Err(err) = store.delete_session(id) {
                self.error = Some(format!("Erro ao excluir sessão: {err:#}"));
            }
        }
        self.reload_tasks();
        self.reload_stats();
        cx.notify();
    }

    pub fn add_manual_session(&mut self, minutes: u32, cx: &mut Context<Self>) {
        let task_id = self.current_task.clone();
        if let Some(store) = &self.store {
            if let Err(err) = store.add_manual_session(minutes, task_id) {
                self.error = Some(format!("Erro ao registrar foco: {err:#}"));
            }
        }
        self.reload_tasks();
        self.reload_stats();
        cx.notify();
    }

    pub fn export_sessions_csv(&self) -> anyhow::Result<String> {
        let store = self
            .store
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Histórico indisponível"))?;
        store.export_sessions_csv()
    }

    fn with_store(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&Store) -> anyhow::Result<()>) {
        if let Some(store) = &self.store {
            if let Err(err) = f(store) {
                self.error = Some(format!("{err:#}"));
            }
        }
        self.reload_tasks();
        cx.notify();
    }

    /// Adds a task with the default estimate (or a trailing `~N` in the title).
    #[allow(dead_code)]
    pub fn add_task(&mut self, title: &str, cx: &mut Context<Self>) {
        self.add_task_with_estimate(title, 1, cx);
    }

    /// Adds a task. A trailing `~N` in the title sets the estimate unless an
    /// explicit `estimate` above 1 was chosen.
    pub fn add_task_with_estimate(&mut self, title: &str, estimate: u32, cx: &mut Context<Self>) {
        let (title, typed) = split_estimate(title);
        if title.is_empty() {
            return;
        }
        let estimate = if estimate > 1 {
            estimate
        } else {
            typed.unwrap_or(estimate)
        };
        let mut added = None;
        self.with_store(cx, |s| {
            added = Some(s.add_task_with_estimate(title, estimate)?.id);
            Ok(())
        });
        if self.current_task.is_none() {
            self.current_task = added;
        }
    }

    pub fn rename_task(&mut self, id: &str, title: &str, cx: &mut Context<Self>) {
        if title.trim().is_empty() {
            return;
        }
        self.with_store(cx, |s| s.rename_task(id, title));
    }

    pub fn move_task(&mut self, id: &str, delta: i32, cx: &mut Context<Self>) {
        self.with_store(cx, |s| s.move_task(id, delta));
    }

    pub fn set_intention(&mut self, text: String, cx: &mut Context<Self>) {
        if self.intention != text {
            self.intention = text;
            cx.notify();
        }
    }

    pub fn set_task_done(&mut self, id: &str, done: bool, cx: &mut Context<Self>) {
        self.with_store(cx, |s| s.set_task_done(id, done));
    }

    pub fn adjust_estimate(&mut self, id: &str, delta: i32, cx: &mut Context<Self>) {
        self.with_store(cx, |s| s.adjust_estimate(id, delta));
    }

    pub fn delete_task(&mut self, id: &str, cx: &mut Context<Self>) {
        self.with_store(cx, |s| s.delete_task(id));
    }

    pub fn clear_done_tasks(&mut self, cx: &mut Context<Self>) {
        self.with_store(cx, |s| s.clear_done_tasks());
    }

    pub fn set_current_task(&mut self, id: Option<String>, cx: &mut Context<Self>) {
        self.current_task = id;
        cx.notify();
    }

    pub fn complete_current_task(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.current_task.clone() else {
            return;
        };
        let next_task = {
            let open: Vec<&TaskItem> = self.open_tasks().collect();
            open.iter()
                .skip_while(|t| t.id != id)
                .nth(1)
                .or_else(|| open.iter().find(|t| t.id != id))
                .map(|t| t.id.clone())
        };
        self.with_store(cx, |s| s.set_task_done(&id, true));
        self.current_task = next_task.filter(|next_id| {
            self.tasks.iter().any(|t| &t.id == next_id && !t.done)
        });
        cx.notify();
    }

    pub fn current_task_title(&self) -> Option<&str> {
        let id = self.current_task.as_ref()?;
        self.tasks
            .iter()
            .find(|t| &t.id == id)
            .map(|t| t.title.as_str())
    }

    pub fn open_tasks(&self) -> impl Iterator<Item = &TaskItem> {
        self.tasks.iter().filter(|t| !t.done)
    }

    // ---- Navigation & misc -------------------------------------------------

    pub fn navigate(&mut self, page: Page, cx: &mut Context<Self>) {
        if self.page != page {
            self.page = page;
            cx.notify();
        }
    }

    pub fn media(&mut self, cmd: MediaCommand, cx: &mut Context<Self>) {
        self.media.send(cmd);
        // Optimistic flip so the button reacts before the next poll.
        if let (MediaCommand::PlayPause, Some(np)) = (cmd, self.now_playing.as_mut()) {
            np.playing = !np.playing;
            cx.notify();
        }
    }

    fn pause_media_for_break(&mut self, cx: &mut Context<Self>) {
        if self.settings.auto_pause_media
            && self.now_playing.as_ref().is_some_and(|np| np.playing)
        {
            self.media(MediaCommand::PlayPause, cx);
            self.media_paused_by_enki = true;
        }
    }

    fn resume_media_for_focus(&mut self, cx: &mut Context<Self>) {
        if !self.media_paused_by_enki {
            return;
        }
        self.media_paused_by_enki = false;
        if self.now_playing.as_ref().is_some_and(|np| !np.playing) {
            self.media(MediaCommand::PlayPause, cx);
        }
    }

    pub fn dismiss_error(&mut self, cx: &mut Context<Self>) {
        self.error = None;
        cx.notify();
    }

    /// "Foco · 2 de 4"
    pub fn phase_caption(&self) -> String {
        let t = &self.timer;
        match t.phase() {
            Phase::Focus => format!("Foco · {} de {}", t.cycle_position(), t.cycle_length()),
            phase => phase.label().to_string(),
        }
    }

    pub fn clock_text(&self) -> String {
        format_clock(self.timer.remaining(Instant::now()))
    }

    fn sync_tray(&mut self) {
        if self.settings.menu_bar && self.tray.is_none() && !self.tray_failed {
            match Tray::new() {
                Ok(tray) => self.tray = Some(tray),
                Err(err) => {
                    eprintln!("enki: tray unavailable: {err:#}");
                    self.tray_failed = true;
                }
            }
        } else if !self.settings.menu_bar {
            self.tray = None;
        }
        if let Some(tray) = &mut self.tray {
            let t = &self.timer;
            let label = if t.is_idle() && t.phase() == Phase::Focus {
                None
            } else {
                Some(format_clock(t.remaining(Instant::now())))
            };
            tray.update(label, t.is_running(), t.phase());
        }
    }
}

#[cfg(debug_assertions)]
impl Enki {
    /// Dev helper for screenshots: `ENKI_DEBUG_PAGE=tasks|stats|settings[:section]`.
    fn apply_debug_env(&mut self) {
        if std::env::var("ENKI_DEBUG_SYNC").is_ok() {
            self.settings.reduce_motion = true;
        }
        let page = std::env::var("ENKI_DEBUG_PAGE").unwrap_or_default();
        let (page, section) = page.split_once(':').unwrap_or((page.as_str(), ""));
        let section = SettingsSection::ALL
            .into_iter()
            .find(|s| s.label().eq_ignore_ascii_case(section))
            .unwrap_or(SettingsSection::General);
        self.page = match page {
            "tasks" => Page::Tasks,
            "stats" => Page::Stats,
            "settings" => Page::Settings(section),
            _ => Page::Timer,
        };
        if let Ok(task) = std::env::var("ENKI_DEBUG_TASK") {
            self.current_task = self
                .tasks
                .iter()
                .find(|t| t.title.contains(&task))
                .map(|t| t.id.clone());
        }
        if std::env::var("ENKI_DEBUG_RUN").is_ok() {
            self.phase_started_at = Some(now());
            self.timer
                .start(Instant::now() - Duration::from_secs(9 * 60 + 23));
        }
    }
}

pub fn is_dark(appearance: WindowAppearance) -> bool {
    matches!(
        appearance,
        WindowAppearance::Dark | WindowAppearance::VibrantDark
    )
}

/// Splits a trailing `~N` estimate shortcut off a task title:
/// `"Revisar PR ~3"` → `("Revisar PR", Some(3))`. The `~` must follow a space
/// and leave a non-empty title, otherwise the text is kept as typed.
fn split_estimate(title: &str) -> (&str, Option<u32>) {
    let title = title.trim();
    let Some((head, digits)) = title.rsplit_once('~') else {
        return (title, None);
    };
    let head_trimmed = head.trim_end();
    let valid = !digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit())
        && head.ends_with(char::is_whitespace)
        && !head_trimmed.is_empty();
    match digits.parse::<u32>() {
        Ok(n) if valid => (head_trimmed, Some(n)),
        _ => (title, None),
    }
}

pub fn init(cx: &mut App) -> Entity<Enki> {
    let model = cx.new(Enki::new);
    let theme = model.read(cx).build_theme();
    cx.set_global(theme);
    cx.set_global(AppModel(model.clone()));
    model
}

#[cfg(test)]
mod tests {
    use super::split_estimate;

    #[test]
    fn estimate_suffix() {
        assert_eq!(split_estimate("Revisar PR ~3"), ("Revisar PR", Some(3)));
        assert_eq!(split_estimate("  Tese   ~12  "), ("Tese", Some(12)));
        assert_eq!(split_estimate("Revisar PR"), ("Revisar PR", None));
        assert_eq!(split_estimate("v1~2"), ("v1~2", None));
        assert_eq!(split_estimate("~3"), ("~3", None));
        assert_eq!(split_estimate("Algo ~"), ("Algo ~", None));
        assert_eq!(split_estimate("Algo ~3a"), ("Algo ~3a", None));
        assert_eq!(
            split_estimate("Algo ~99999999999"),
            ("Algo ~99999999999", None)
        );
        assert_eq!(split_estimate("   "), ("", None));
    }
}
