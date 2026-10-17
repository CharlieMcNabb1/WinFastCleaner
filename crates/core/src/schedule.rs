//! Scheduled cleaning: the schedule (part of the settings), the Windows Task
//! Scheduler task that runs it, and the run itself.
//!
//! A scheduled run only uses cleaning rules: safe (green) items by default,
//! optionally also optional (yellow) items, which go to the recovery bin.
//! Red items are never part of it. It runs as the signed-in user without
//! administrator rights, so rules that need admin are skipped and reported,
//! as are rules whose app is running. Nobody is asked anything: everything
//! the run did is in the log and in its summary (`scheduled-last.json`).

use std::collections::HashSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::cleaner::{self, CleanReport, Context, ExecOptions, RuleStatus};
use crate::progress::CancelToken;
use crate::recovery::RecoveryStore;
use crate::rules::{self, Rule, Tier};
use crate::settings::{Exclusions, Settings};

/// Task Scheduler folder and task name.
pub const TASK_FOLDER: &str = r"\WinFastCleaner";
pub const TASK_NAME: &str = "Scheduled cleaning";
/// The program the task runs (the CLI without a console window) and its
/// fixed arguments.
pub const TASK_PROGRAM: &str = "wfcw.exe";
pub const TASK_ARGUMENTS: &str = "clean --scheduled";

const LAST_FILE: &str = "scheduled-last.json";
const MAX_RULES: usize = 500;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Frequency {
    Daily,
    Weekly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Weekday {
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
    Sunday,
}

impl Weekday {
    fn xml_name(self) -> &'static str {
        match self {
            Weekday::Monday => "Monday",
            Weekday::Tuesday => "Tuesday",
            Weekday::Wednesday => "Wednesday",
            Weekday::Thursday => "Thursday",
            Weekday::Friday => "Friday",
            Weekday::Saturday => "Saturday",
            Weekday::Sunday => "Sunday",
        }
    }
}

/// When and what to clean automatically. Off by default.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Schedule {
    pub enabled: bool,
    pub frequency: Frequency,
    /// For weekly runs.
    pub weekday: Weekday,
    /// Local time of day.
    pub hour: u8,
    pub minute: u8,
    /// Also clean optional (yellow) items. They go to the recovery bin.
    pub include_optional: bool,
    /// Only these rules. Empty means every rule in the chosen tiers.
    pub rules: Vec<String>,
}

impl Default for Schedule {
    fn default() -> Self {
        Schedule {
            enabled: false,
            frequency: Frequency::Weekly,
            weekday: Weekday::Sunday,
            hour: 12,
            minute: 0,
            include_optional: false,
            rules: Vec::new(),
        }
    }
}

impl Schedule {
    pub fn validate(&self) -> Result<(), String> {
        if self.hour > 23 || self.minute > 59 {
            return Err("the time of day is not valid".into());
        }
        if self.rules.len() > MAX_RULES {
            return Err(format!("at most {MAX_RULES} rules can be picked"));
        }
        let id_ok = |id: &str| {
            (1..=64).contains(&id.len())
                && id
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-')
        };
        if let Some(bad) = self.rules.iter().find(|r| !id_ok(r)) {
            return Err(format!("`{bad}` is not a rule id"));
        }
        Ok(())
    }

    /// The highest tier a run may clean. Never red.
    pub fn max_tier(&self) -> Tier {
        if self.include_optional {
            Tier::Yellow
        } else {
            Tier::Green
        }
    }
}

/// The rules a scheduled run uses: those in the chosen tiers (and, if the
/// schedule names rules, only those). Red never qualifies.
pub fn select_rules(all: &[Rule], s: &Schedule) -> Vec<Rule> {
    let max = s.max_tier();
    all.iter()
        .filter(|r| r.tier != Tier::Red && r.tier <= max)
        .filter(|r| s.rules.is_empty() || s.rules.iter().any(|id| id == &r.id))
        .cloned()
        .collect()
}

// ------------------------------------------------------------ the task

fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// Task Scheduler XML for the schedule. The trigger's start date is in the
/// past and has no time zone, so the task runs at the chosen local time
/// from now on (also across daylight saving changes). A missed run (the PC
/// was off) runs as soon as possible afterwards. The task runs as the
/// signed-in user with the least privileges.
pub fn task_xml(s: &Schedule, program: &Path, arguments: &str) -> String {
    let when = match s.frequency {
        Frequency::Daily => {
            "<ScheduleByDay><DaysInterval>1</DaysInterval></ScheduleByDay>".to_string()
        }
        Frequency::Weekly => format!(
            "<ScheduleByWeek><DaysOfWeek><{0} /></DaysOfWeek><WeeksInterval>1</WeeksInterval></ScheduleByWeek>",
            s.weekday.xml_name()
        ),
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Author>WinFastCleaner</Author>
    <Description>Cleans junk files on a schedule (safe items only, unless you chose otherwise). Created by WinFastCleaner; change or turn it off in WinFastCleaner's Settings.</Description>
  </RegistrationInfo>
  <Triggers>
    <CalendarTrigger>
      <StartBoundary>2000-01-01T{hour:02}:{minute:02}:00</StartBoundary>
      <Enabled>true</Enabled>
      {when}
    </CalendarTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>LeastPrivilege</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <StartWhenAvailable>true</StartWhenAvailable>
    <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>false</Hidden>
    <ExecutionTimeLimit>PT1H</ExecutionTimeLimit>
    <Priority>7</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{program}</Command>
      <Arguments>{arguments}</Arguments>
    </Exec>
  </Actions>
</Task>
"#,
        hour = s.hour.min(23),
        minute = s.minute.min(59),
        program = xml_escape(&program.display().to_string()),
        arguments = xml_escape(arguments),
    )
}

/// Creates, updates, or removes the task to match the schedule. `program`
/// is the full path of `wfcw.exe`.
#[cfg(windows)]
pub fn apply(s: &Schedule, program: &Path) -> Result<(), String> {
    if s.enabled {
        s.validate()?;
        if !program.is_absolute() || !program.is_file() {
            return Err(format!(
                "The scheduled cleaner ({TASK_PROGRAM}) is missing next to the app. Reinstall WinFastCleaner."
            ));
        }
        crate::wincom::register_task(
            TASK_FOLDER,
            TASK_NAME,
            &task_xml(s, program, TASK_ARGUMENTS),
        )
    } else {
        crate::wincom::delete_task(TASK_FOLDER, TASK_NAME)
    }
}

/// Whether the task exists (and is turned on in Task Scheduler).
#[cfg(windows)]
pub fn task_state() -> Result<Option<bool>, String> {
    crate::wincom::task_state(TASK_FOLDER, TASK_NAME)
}

// ------------------------------------------------------------ the run

/// A rule a run skipped, and why.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotReady {
    pub rule: String,
    pub reason: String,
}

/// What the last scheduled run did.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LastRun {
    pub started_at: i64,
    pub finished_at: i64,
    /// False when the run couldn't do its job at all.
    pub ok: bool,
    pub message: String,
    pub deleted_count: u64,
    pub deleted_bytes: u64,
    pub binned_count: u64,
    pub binned_bytes: u64,
    pub skipped_count: u64,
    pub freed_bytes: u64,
    pub batch_id: Option<String>,
    pub not_ready: Vec<NotReady>,
}

pub fn save_last(data_dir: &Path, r: &LastRun) -> std::io::Result<()> {
    let json = serde_json::to_vec_pretty(r).map_err(std::io::Error::other)?;
    crate::paths::write_atomic(&data_dir.join(LAST_FILE), &json)
}

pub fn load_last(data_dir: &Path) -> Option<LastRun> {
    let bytes = std::fs::read(data_dir.join(LAST_FILE)).ok()?;
    if bytes.len() > 256 * 1024 {
        return None;
    }
    serde_json::from_slice(&bytes).ok()
}

fn reason(s: &RuleStatus) -> String {
    match s {
        RuleStatus::Ready | RuleStatus::NothingFound => String::new(),
        RuleStatus::NeedsAdmin => "needs administrator rights".into(),
        RuleStatus::AppRunning(apps) => format!("{} was open", apps.join(", ")),
    }
}

/// Runs the scheduled clean now, with the schedule in the settings, and
/// records what it did. `rules_dir` replaces the rules (built-in and the
/// user's) with one folder's rules, for tests.
pub fn run(data_dir: &Path, rules_dir: Option<&Path>, cancel: &CancelToken) -> LastRun {
    let started_at = crate::scan::now_unix();
    let mut last = run_inner(data_dir, rules_dir, cancel);
    last.started_at = started_at;
    last.finished_at = crate::scan::now_unix();
    let _ = save_last(data_dir, &last);
    last
}

fn run_inner(data_dir: &Path, rules_dir: Option<&Path>, cancel: &CancelToken) -> LastRun {
    let settings = Settings::load(data_dir);
    let s = &settings.schedule;
    if !s.enabled {
        return LastRun {
            ok: true,
            message: "Scheduled cleaning is turned off, so nothing was cleaned.".into(),
            ..LastRun::default()
        };
    }
    if let Err(e) = s.validate() {
        return LastRun {
            message: format!("The schedule is not valid: {e}."),
            ..LastRun::default()
        };
    }
    let ctx = Context::system(Exclusions::new(&settings.exclusions));
    let set = match rules_dir {
        Some(dir) => rules::load_dir(dir, &ctx.vars, &ctx.guard),
        None => rules::load(
            Some(&crate::paths::user_rules_dir(data_dir)),
            &ctx.vars,
            &ctx.guard,
        ),
    };
    let selected = select_rules(&set.rules, s);
    if selected.is_empty() {
        return LastRun {
            ok: true,
            message: "No cleaning rules match the schedule, so nothing was cleaned.".into(),
            ..LastRun::default()
        };
    }
    let ids: Vec<String> = selected.iter().map(|r| r.id.clone()).collect();
    let scan = cleaner::scan(&selected, &ctx, cancel);
    let mut plan = cleaner::plan_rules(&scan, &ids, &HashSet::new());
    // Rules can't be red, but a scheduled run must never act on red items,
    // so this is checked here too.
    plan.items
        .retain(|i| i.tier != Tier::Red && i.tier <= s.max_tier());
    plan.total_size = plan.items.iter().map(|i| i.size).sum();
    let not_ready: Vec<NotReady> = plan
        .not_ready
        .iter()
        .map(|(id, status)| NotReady {
            rule: scan
                .rules
                .iter()
                .find(|r| &r.rule.id == id)
                .map(|r| r.rule.name.clone())
                .unwrap_or_else(|| id.clone()),
            reason: reason(status),
        })
        .collect();

    let report = if plan.items.is_empty() {
        CleanReport::default()
    } else {
        let store = RecoveryStore::open(data_dir, settings.bin);
        let opts = ExecOptions {
            green_to_bin: settings.green_to_bin,
            label: "Scheduled cleaning".into(),
            secure: false,
        };
        cleaner::execute(&plan, &ctx, &store, data_dir, &opts, &|_, _| {}, cancel)
    };
    let cleaned = report.deleted_bytes + report.binned_bytes;
    let files = report.deleted_count + report.binned_count;
    let mut message = if files == 0 {
        "Nothing to clean.".to_string()
    } else {
        format!(
            "Cleaned {} ({} {}).",
            crate::format::format_size(cleaned),
            crate::format::format_count(files),
            if files == 1 { "file" } else { "files" }
        )
    };
    if !not_ready.is_empty() {
        message.push_str(&format!(
            " Skipped {} {} (see the details).",
            not_ready.len(),
            if not_ready.len() == 1 {
                "rule"
            } else {
                "rules"
            }
        ));
    }
    LastRun {
        ok: true,
        message,
        deleted_count: report.deleted_count,
        deleted_bytes: report.deleted_bytes,
        binned_count: report.binned_count,
        binned_bytes: report.binned_bytes,
        skipped_count: report.skipped_count,
        freed_bytes: report.freed_bytes,
        batch_id: report.batch_id,
        not_ready,
        ..LastRun::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::RuleSource;

    /// Rule files can't make red rules; this builds rules by hand so the
    /// test can show a schedule refuses red anyway.
    fn rule(id: &str, tier: Tier) -> Rule {
        Rule {
            id: id.into(),
            name: id.into(),
            group: "Test".into(),
            tier,
            description: "d".into(),
            lose: "l".into(),
            paths: vec![r"C:\Temp\x".into()],
            include: Vec::new(),
            exclude: Vec::new(),
            min_age_days: None,
            requires_admin: false,
            close_apps: Vec::new(),
            remove_empty_folders: false,
            source: RuleSource::BuiltIn("test".into()),
        }
    }

    #[test]
    fn only_rules_in_the_chosen_tiers_and_never_red() {
        let all = vec![
            rule("a.green", Tier::Green),
            rule("b.yellow", Tier::Yellow),
            rule("c.red", Tier::Red),
        ];
        let mut s = Schedule::default();
        let ids = |s: &Schedule| -> Vec<String> {
            select_rules(&all, s).into_iter().map(|r| r.id).collect()
        };
        assert_eq!(ids(&s), ["a.green"]);
        s.include_optional = true;
        assert_eq!(ids(&s), ["a.green", "b.yellow"]);
        s.rules = vec!["b.yellow".into(), "c.red".into()];
        assert_eq!(ids(&s), ["b.yellow"]);
        s.include_optional = false;
        assert!(ids(&s).is_empty());
    }

    #[test]
    fn schedules_are_validated() {
        assert!(Schedule::default().validate().is_ok());
        for bad in [
            Schedule {
                hour: 24,
                ..Schedule::default()
            },
            Schedule {
                minute: 60,
                ..Schedule::default()
            },
            Schedule {
                rules: vec!["../x".into()],
                ..Schedule::default()
            },
            Schedule {
                rules: vec!["Upper".into()],
                ..Schedule::default()
            },
        ] {
            assert!(bad.validate().is_err(), "{bad:?}");
        }
    }

    #[test]
    fn task_xml_is_escaped_and_uses_local_time() {
        let s = Schedule {
            enabled: true,
            frequency: Frequency::Weekly,
            weekday: Weekday::Wednesday,
            hour: 7,
            minute: 5,
            ..Schedule::default()
        };
        let xml = task_xml(
            &s,
            Path::new(r"C:\Program Files\Win & Fast <Cleaner>\wfcw.exe"),
            TASK_ARGUMENTS,
        );
        assert!(xml.contains("<StartBoundary>2000-01-01T07:05:00</StartBoundary>"));
        assert!(xml.contains("<Wednesday />"));
        assert!(xml.contains(
            r"<Command>C:\Program Files\Win &amp; Fast &lt;Cleaner&gt;\wfcw.exe</Command>"
        ));
        assert!(xml.contains("<Arguments>clean --scheduled</Arguments>"));
        assert!(xml.contains("<RunLevel>LeastPrivilege</RunLevel>"));
        assert!(xml.contains("<LogonType>InteractiveToken</LogonType>"));
        let daily = task_xml(
            &Schedule {
                frequency: Frequency::Daily,
                ..s
            },
            Path::new(r"C:\a.exe"),
            "x",
        );
        assert!(daily.contains("<DaysInterval>1</DaysInterval>"));
        assert!(!daily.contains("ScheduleByWeek"));
    }

    #[test]
    fn turned_off_schedules_clean_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let last = run(tmp.path(), Some(tmp.path()), &CancelToken::new());
        assert!(last.ok);
        assert!(last.message.contains("turned off"));
        assert_eq!(load_last(tmp.path()), Some(last));
    }
}
