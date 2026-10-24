//! Automatic backups before changes.
//!
//! - Risky changes (config write, restore, import) always get a backup first, and are refused
//!   if that backup fails.
//! - Bulk deletes and scans that change data get a backup, but are not blocked by a failure
//!   (soft deletes and sync are reversible through trash and history).
//! - Ordinary edits get at most one backup per `auto_backup_interval_min`.
//!
//! Nothing happens without a remembered password or with automatic backups turned off.

use age::secrecy::SecretString;
use chrono::{DateTime, Utc};

use super::{BackupInfo, Ctx, Reason, create, list, prune};
use crate::error::AppResult;

impl Reason {
    /// Must the change wait for (and be refused without) a successful backup?
    pub fn strict(self) -> bool {
        matches!(
            self,
            Reason::ConfigWrite | Reason::BeforeRestore | Reason::Import
        )
    }
}

/// Makes the automatic backup for `reason` if one is due. `Ok(None)` = not needed or not
/// possible (off, or no remembered password).
pub fn before_change(ctx: &Ctx, reason: Reason) -> AppResult<Option<BackupInfo>> {
    let settings = ctx.settings()?;
    if !settings.auto_backup {
        return Ok(None);
    }
    let Some(password) = ctx.secrets.get()? else {
        return Ok(None);
    };
    let dir = ctx.backup_dir()?;
    if reason == Reason::Periodic {
        if settings.auto_backup_interval_min == 0 {
            return Ok(None);
        }
        let due =
            Utc::now() - chrono::Duration::minutes(i64::from(settings.auto_backup_interval_min));
        let recent = list(&dir)?
            .first()
            .is_some_and(|b| DateTime::parse_from_rfc3339(&b.created_at).is_ok_and(|t| t > due));
        if recent {
            return Ok(None);
        }
    }
    let info = create(ctx, &SecretString::from(password), reason)?;
    let pruned = prune(&dir, &settings)?;
    if pruned > 0 {
        tracing::info!(pruned, "old automatic backups removed");
    }
    Ok(Some(info))
}
