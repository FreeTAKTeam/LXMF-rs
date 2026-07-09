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
}
