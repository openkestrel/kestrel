use anyhow::Result;
use jiff::Timestamp;
use sqlx::SqliteConnection;

use crate::store::{due, timestamp};
use crate::work::GAP;

pub struct LeaseSweep<'a> {
    connection: &'a mut SqliteConnection,
}

#[derive(Clone, Copy)]
pub struct Gap {
    pub start: Timestamp,
    pub end: Timestamp,
}

impl<'a> LeaseSweep<'a> {
    pub(crate) fn over(connection: &'a mut SqliteConnection) -> Self {
        Self { connection }
    }

    pub async fn record_pass(&mut self, now: Timestamp) -> Result<Option<Gap>> {
        let last = sqlx::query("SELECT last_pass_at FROM lease_sweep WHERE id = 1")
            .fetch_optional(&mut *self.connection)
            .await?
            .map(|row| timestamp(&row, "last_pass_at"))
            .transpose()?
            .flatten();
        let gap = last
            .filter(|at| now > *at + GAP)
            .map(|start| Gap { start, end: now });
        sqlx::query(
            "INSERT INTO lease_sweep (id, last_pass_at, gap_start, gap_end) VALUES (1, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET last_pass_at = excluded.last_pass_at,
                gap_start = COALESCE(excluded.gap_start, lease_sweep.gap_start),
                gap_end = COALESCE(excluded.gap_end, lease_sweep.gap_end)",
        )
        .bind(due(now))
        .bind(gap.map(|gap| due(gap.start)))
        .bind(gap.map(|gap| due(gap.end)))
        .execute(&mut *self.connection)
        .await?;
        Ok(gap)
    }

    pub async fn gap(&mut self) -> Result<Option<Gap>> {
        let Some(row) = sqlx::query("SELECT gap_start, gap_end FROM lease_sweep WHERE id = 1")
            .fetch_optional(&mut *self.connection)
            .await?
        else {
            return Ok(None);
        };
        Ok(timestamp(&row, "gap_start")?
            .zip(timestamp(&row, "gap_end")?)
            .map(|(start, end)| Gap { start, end }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;
    use jiff::SignedDuration;
    use tempfile::TempDir;

    #[tokio::test]
    async fn only_a_pass_more_than_ten_seconds_late_records_a_gap_and_it_survives_restart() {
        let directory = TempDir::new().unwrap();
        let store = Store::open(directory.path()).await.unwrap();
        let start: Timestamp = "2026-10-01T12:00:00Z".parse().unwrap();
        let mut tx = store.begin().await.unwrap();
        assert!(tx.lease_sweep().record_pass(start).await.unwrap().is_none());
        let next = start + SignedDuration::from_secs(10);
        assert!(tx.lease_sweep().record_pass(next).await.unwrap().is_none());
        let end = next + SignedDuration::from_secs(11);
        let gap = tx.lease_sweep().record_pass(end).await.unwrap().unwrap();
        assert_eq!(gap.start, next);
        assert_eq!(gap.end, end);
        tx.commit().await.unwrap();
        drop(store);
        let store = Store::open(directory.path()).await.unwrap();
        let mut tx = store.begin().await.unwrap();
        assert!(
            tx.lease_sweep()
                .record_pass(end + SignedDuration::from_secs(1))
                .await
                .unwrap()
                .is_none()
        );
        let gap = tx.lease_sweep().gap().await.unwrap().unwrap();
        assert_eq!(gap.start, next);
        assert_eq!(gap.end, end);
        tx.commit().await.unwrap();
    }
}
