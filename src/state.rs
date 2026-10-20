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
    store::{now, SessionRecord, Stats, Store, Task as TaskItem},
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

const TICK: Duration = Duration::from_millis(250);

pub struct Enki {
    pub settings: Settings,
    pub timer: Timer,
    store: Option<Store>,
    pub tasks: Vec<TaskItem>,
    pub stats: Stats,
    /// The local day `stats` was computed for, to refresh "today" after midnight.
    stats_day: NaiveDate,
    pub current_task: Option<String>,
    pub intention: String,
    pub page: Page,
    pub wallpaper: Option<Wallpaper>,
    pub now_playing: Option<NowPlaying>,
    pub system_dark: bool,
    pub error: Option<String>,
    /// Wall-clock start of the phase currently on the clock, for the history.
    phase_started_at: Option<i64>,
    media: MediaWatcher,
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
            }
        }
        if s.menu_bar != before.menu_bar {
            self.tray_failed = false;
            self.sync_tray();
        }
        if s.background_image != before.background_image
            || s.background_effect != before.background_effect
        {
            self.load_wallpaper(cx);
        }
        self.apply_theme(cx);
        cx.notify();
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
        if self.timer.is_idle() {
            self.phase_started_at = Some(crate::store::now());
        }
        self.timer.toggle(now);
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
            self.record(finished);
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
            self.record(Finished {
                phase: self.timer.phase(),
                elapsed,
                completed: false,
            });
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
        let intention = self.intention.trim();
        let record = SessionRecord {
            phase: finished.phase,
            started_at,
            ended_at,
            seconds: finished.elapsed.as_secs(),
            completed: finished.completed,
            task_id: self.current_task.clone(),
            intention: (!intention.is_empty()).then(|| intention.to_string()),
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
        }
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

    pub fn add_task(&mut self, title: &str, cx: &mut Context<Self>) {
        if title.trim().is_empty() {
            return;
        }
        let mut added = None;
        self.with_store(cx, |s| {
            added = Some(s.add_task(title)?.id);
            Ok(())
        });
        if self.current_task.is_none() {
            self.current_task = added;
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

pub fn init(cx: &mut App) -> Entity<Enki> {
    let model = cx.new(Enki::new);
    let theme = model.read(cx).build_theme();
    cx.set_global(theme);
    cx.set_global(AppModel(model.clone()));
    model
}
