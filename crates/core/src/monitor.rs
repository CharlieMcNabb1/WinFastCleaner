//! What the tray monitor tells the user, and how often.
//!
//! The tray runs the health check every hour and turns it into
//! notifications: a drive below the free-space threshold, junk above the
//! junk threshold, or a check that turned red. Each kind is sent at most
//! once a day. The state (when each kind was last sent, which checks were
//! red) lives in `tray-state.json` in the data folder.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::health::{Check, CheckId, Status};

const STATE_FILE: &str = "tray-state.json";
const DAY: i64 = 24 * 60 * 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AlertKind {
    LowSpace,
    Junk,
    HealthRed,
}

/// One notification.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Alert {
    pub kind: AlertKind,
    pub title: String,
    pub body: String,
    /// A warning (red) rather than information.
    pub warning: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct MonitorState {
    pub low_space_sent: Option<i64>,
    pub junk_sent: Option<i64>,
    pub health_red_sent: Option<i64>,
    /// Checks that were red last time, so only newly red ones are told.
    pub red: Vec<CheckId>,
    /// When the tray last scanned for junk itself.
    pub junk_scanned: Option<i64>,
}

impl MonitorState {
    pub fn load(data_dir: &Path) -> MonitorState {
        std::fs::read(data_dir.join(STATE_FILE))
            .ok()
            .filter(|b| b.len() < 64 * 1024)
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, data_dir: &Path) -> std::io::Result<()> {
        let json = serde_json::to_vec(self).map_err(std::io::Error::other)?;
        crate::paths::write_atomic(&data_dir.join(STATE_FILE), &json)
    }

    fn sent(&mut self, kind: AlertKind) -> &mut Option<i64> {
        match kind {
            AlertKind::LowSpace => &mut self.low_space_sent,
            AlertKind::Junk => &mut self.junk_sent,
            AlertKind::HealthRed => &mut self.health_red_sent,
        }
    }

    /// Records the send unless one of this kind went out in the last day.
    fn allow(&mut self, kind: AlertKind, now: i64) -> bool {
        let last = self.sent(kind);
        // A clock set back shouldn't silence alerts forever.
        if last.is_some_and(|t| t <= now && now - t < DAY) {
            return false;
        }
        *last = Some(now);
        true
    }
}

fn status_of(checks: &[Check], id: CheckId) -> Option<&Check> {
    checks.iter().find(|c| c.id == id)
}

/// The notifications due now, given the latest health check. Updates the
/// state (call [`MonitorState::save`] afterwards).
pub fn alerts(checks: &[Check], state: &mut MonitorState, now: i64) -> Vec<Alert> {
    let mut out = Vec::new();
    let attention = |c: &Check| matches!(c.status, Status::Yellow | Status::Red);

    if let Some(c) = status_of(checks, CheckId::DiskSpace).filter(|c| attention(c))
        && state.allow(AlertKind::LowSpace, now)
    {
        out.push(Alert {
            kind: AlertKind::LowSpace,
            title: "Low on disk space".into(),
            body: c.summary.clone(),
            warning: c.status == Status::Red,
        });
    }
    if let Some(c) = status_of(checks, CheckId::Junk).filter(|c| attention(c))
        && state.allow(AlertKind::Junk, now)
    {
        out.push(Alert {
            kind: AlertKind::Junk,
            title: "Junk files are piling up".into(),
            body: format!(
                "{} Open WinFastCleaner to review and clean them.",
                c.summary
            ),
            warning: false,
        });
    }

    // Other checks: only when they turn red (disk space and junk are
    // covered above).
    let red_now: Vec<CheckId> = checks
        .iter()
        .filter(|c| c.status == Status::Red)
        .map(|c| c.id)
        .collect();
    let newly_red: Vec<&Check> = checks
        .iter()
        .filter(|c| c.status == Status::Red && !state.red.contains(&c.id))
        .filter(|c| !matches!(c.id, CheckId::DiskSpace | CheckId::Junk))
        .collect();
    if !newly_red.is_empty() && state.allow(AlertKind::HealthRed, now) {
        let body = newly_red
            .iter()
            .map(|c| format!("{}: {}", c.title, c.summary))
            .collect::<Vec<_>>()
            .join(" ");
        out.push(Alert {
            kind: AlertKind::HealthRed,
            title: "The health check found a problem".into(),
            body,
            warning: true,
        });
    }
    state.red = red_now;
    out
}

/// Shortens text to what a Windows notification shows (`max` characters).
pub fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(id: CheckId, status: Status) -> Check {
        Check {
            id,
            title: format!("{id:?}"),
            status,
            summary: format!("{id:?} is {status:?}."),
            details: Vec::new(),
            fix: None,
            as_of: None,
        }
    }

    const T0: i64 = 1_800_000_000;

    #[test]
    fn low_space_and_junk_are_sent_at_most_once_a_day() {
        let mut st = MonitorState::default();
        let checks = vec![
            check(CheckId::DiskSpace, Status::Yellow),
            check(CheckId::Junk, Status::Red),
        ];
        let a = alerts(&checks, &mut st, T0);
        assert_eq!(
            a.iter().map(|a| a.kind).collect::<Vec<_>>(),
            [AlertKind::LowSpace, AlertKind::Junk]
        );
        assert!(alerts(&checks, &mut st, T0 + 3600).is_empty());
        assert!(alerts(&checks, &mut st, T0 + DAY - 1).is_empty());
        assert_eq!(alerts(&checks, &mut st, T0 + DAY).len(), 2);
        // A clock set back doesn't block alerts for good.
        assert_eq!(alerts(&checks, &mut st, T0 - 10 * DAY).len(), 2);
    }

    #[test]
    fn green_checks_send_nothing() {
        let mut st = MonitorState::default();
        let checks = vec![
            check(CheckId::DiskSpace, Status::Green),
            check(CheckId::Junk, Status::Green),
            check(CheckId::Temperatures, Status::Unknown),
        ];
        assert!(alerts(&checks, &mut st, T0).is_empty());
    }

    #[test]
    fn other_checks_alert_only_when_they_turn_red() {
        let mut st = MonitorState::default();
        let mut checks = vec![check(CheckId::DriveHealth, Status::Red)];
        let a = alerts(&checks, &mut st, T0);
        assert_eq!(a.len(), 1);
        assert!(a[0].warning);
        assert!(a[0].body.contains("DriveHealth"));
        // Still red the next day: not new, so no alert.
        assert!(alerts(&checks, &mut st, T0 + 2 * DAY).is_empty());
        // Back to green, then red again: new.
        checks[0].status = Status::Green;
        assert!(alerts(&checks, &mut st, T0 + 3 * DAY).is_empty());
        checks[0].status = Status::Red;
        assert_eq!(alerts(&checks, &mut st, T0 + 4 * DAY).len(), 1);
        // A red disk is told as low space, not twice.
        let mut st = MonitorState::default();
        let a = alerts(&[check(CheckId::DiskSpace, Status::Red)], &mut st, T0);
        assert_eq!(
            a.iter().map(|a| a.kind).collect::<Vec<_>>(),
            [AlertKind::LowSpace]
        );
    }

    #[test]
    fn state_round_trips_and_text_is_clipped() {
        let tmp = tempfile::tempdir().unwrap();
        let mut st = MonitorState::default();
        alerts(&[check(CheckId::Startup, Status::Red)], &mut st, T0);
        st.save(tmp.path()).unwrap();
        assert_eq!(MonitorState::load(tmp.path()), st);
        assert_eq!(clip("abcdef", 4), "abc…");
        assert_eq!(clip("abc", 4), "abc");
    }
}
