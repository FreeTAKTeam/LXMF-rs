mod tests {
    use super::*;

    #[test]
    fn link_check_delay_uses_retry_deadline_when_sooner_than_default_sweep() {
        let now = std::time::Instant::now();
        let deadline = now + Duration::from_millis(150);

        assert_eq!(link_check_delay_from_deadline(now, Some(deadline)), Duration::from_millis(150));
    }

    #[test]
    fn link_check_delay_clamps_overdue_retries_to_minimum_delay() {
        let now = std::time::Instant::now();
        let deadline = now - Duration::from_millis(5);

        assert_eq!(link_check_delay_from_deadline(now, Some(deadline)), MIN_LINKS_CHECK_DELAY);
    }

    #[test]
    fn link_check_delay_keeps_default_sweep_without_pending_retries() {
        let now = std::time::Instant::now();

        assert_eq!(link_check_delay_from_deadline(now, None), INTERVAL_LINKS_CHECK);
    }

    /// Issue #525: a worker that silently returns before shutdown must
    /// cancel the remaining workers instead of degrading invisibly.
    #[tokio::test]
    async fn supervision_cancels_remaining_workers_when_one_exits_early() {
        use tokio_util::sync::CancellationToken;

        let cancel = CancellationToken::new();
        let mut workers = WorkerSet::new();
        let mut worker_names = WorkerNames::new();

        spawn_named_worker(&mut workers, &mut worker_names, "faulty", async {});
        {
            let cancel = cancel.clone();
            spawn_named_worker(&mut workers, &mut worker_names, "long-lived", async move {
                cancel.cancelled().await;
            });
        }

        tokio::time::timeout(
            Duration::from_secs(2),
            supervise_workers(&mut workers, &worker_names, &cancel),
        )
        .await
        .expect("supervision should drain all workers after early exit");

        assert!(cancel.is_cancelled(), "early worker exit must cancel the transport");
        assert!(workers.is_empty());
    }

    /// Issue #525: a panicking worker must surface as a failure and cancel
    /// the remaining workers.
    #[tokio::test]
    async fn supervision_cancels_remaining_workers_when_one_panics() {
        use tokio_util::sync::CancellationToken;

        let cancel = CancellationToken::new();
        let mut workers = WorkerSet::new();
        let mut worker_names = WorkerNames::new();

        spawn_named_worker(&mut workers, &mut worker_names, "panicky", async {
            panic!("boom")
        });
        {
            let cancel = cancel.clone();
            spawn_named_worker(&mut workers, &mut worker_names, "long-lived", async move {
                cancel.cancelled().await;
            });
        }

        tokio::time::timeout(
            Duration::from_secs(2),
            supervise_workers(&mut workers, &worker_names, &cancel),
        )
        .await
        .expect("supervision should drain all workers after a panic");

        assert!(cancel.is_cancelled(), "worker panic must cancel the transport");
        assert!(workers.is_empty());
    }

    /// Issue #539 review follow-up: worker names are tracked by task id,
    /// so the failure log can attribute a panic (which returns no value)
    /// to the worker that caused it.
    #[tokio::test]
    async fn worker_names_attribute_failures_by_task_id() {
        let mut workers = WorkerSet::new();
        let mut worker_names = WorkerNames::new();

        spawn_named_worker(&mut workers, &mut worker_names, "first", async {});
        spawn_named_worker(&mut workers, &mut worker_names, "second", async {});

        let (id, result) = workers
            .join_next_with_id()
            .await
            .expect("a completed worker")
            .expect("worker finished without panicking");
        assert_eq!(result, ());
        assert!(
            matches!(worker_names.get(&id).copied(), Some("first" | "second")),
            "completed task id must map back to its worker name"
        );
    }

    /// Issue #525: on normal shutdown all workers exit after cancellation
    /// and supervision drains quietly without re-cancelling or hanging.
    #[tokio::test]
    async fn supervision_drains_quietly_on_normal_shutdown() {
        use tokio_util::sync::CancellationToken;

        let cancel = CancellationToken::new();
        let mut workers = WorkerSet::new();
        let mut worker_names = WorkerNames::new();
        for idx in 0..3 {
            let cancel = cancel.clone();
            let name = match idx {
                0 => "worker-a",
                1 => "worker-b",
                _ => "worker-c",
            };
            spawn_named_worker(&mut workers, &mut worker_names, name, async move {
                cancel.cancelled().await;
            });
        }

        cancel.cancel();
        tokio::time::timeout(
            Duration::from_secs(2),
            supervise_workers(&mut workers, &worker_names, &cancel),
        )
        .await
        .expect("supervision should drain cleanly on normal shutdown");
        assert!(workers.is_empty());
    }
}
