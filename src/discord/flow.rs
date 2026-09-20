use std::future::Future;

use tokio::sync::{Semaphore, SemaphorePermit};

use crate::decision::{DecisionRequest, InputError, RawDecisionInput};

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Rejection {
    WrongGuild,
    Input(InputError),
    Busy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DeliveryError {
    Defer,
    Edit,
}

pub(super) fn admit(
    guild: Option<u64>,
    expected_guild: u64,
    raw: RawDecisionInput,
    slots: &Semaphore,
) -> Result<(DecisionRequest, SemaphorePermit<'_>), Rejection> {
    if guild != Some(expected_guild) {
        return Err(Rejection::WrongGuild);
    }
    let request = DecisionRequest::try_from(raw).map_err(Rejection::Input)?;
    let permit = slots.try_acquire().map_err(|_| Rejection::Busy)?;
    Ok((request, permit))
}

pub(super) async fn run_deferred<T, F, Edit>(
    permit: SemaphorePermit<'_>,
    defer: impl Future<Output = Result<(), ()>>,
    evaluate: impl Future<Output = T>,
    finish: F,
) -> Result<(), DeliveryError>
where
    F: FnOnce(T) -> Edit,
    Edit: Future<Output = Result<(), ()>>,
{
    defer.await.map_err(|()| DeliveryError::Defer)?;
    let result = evaluate.await;
    // Keep the permit through all provider retries, but not Discord delivery.
    drop(permit);
    finish(result).await.map_err(|()| DeliveryError::Edit)
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, future::pending};

    use super::*;

    fn raw() -> RawDecisionInput {
        RawDecisionInput {
            question: "Which?".into(),
            a: "First".into(),
            b: "Second".into(),
            c: None,
            d: None,
            context: None,
        }
    }

    #[test]
    fn wrong_guild_and_invalid_inputs_never_reserve_a_slot() {
        let slots = Semaphore::new(2);
        assert!(matches!(
            admit(None, 1, raw(), &slots),
            Err(Rejection::WrongGuild)
        ));
        assert!(matches!(
            admit(Some(2), 1, raw(), &slots),
            Err(Rejection::WrongGuild)
        ));
        let mut invalid = raw();
        invalid.a = " ".into();
        assert!(matches!(
            admit(Some(1), 1, invalid, &slots),
            Err(Rejection::Input(_))
        ));
        assert_eq!(slots.available_permits(), 2);
    }

    #[test]
    fn full_capacity_rejects_immediately_and_dropped_permits_restore_capacity() {
        let slots = Semaphore::new(2);
        let (_, first) = admit(Some(1), 1, raw(), &slots).unwrap();
        let (_, second) = admit(Some(1), 1, raw(), &slots).unwrap();
        assert!(matches!(
            admit(Some(1), 1, raw(), &slots),
            Err(Rejection::Busy)
        ));
        drop(first);
        assert!(admit(Some(1), 1, raw(), &slots).is_ok());
        drop(second);
        assert_eq!(slots.available_permits(), 2);
    }

    #[tokio::test]
    async fn defer_failure_never_polls_provider_or_edits_and_releases_permit() {
        let slots = Semaphore::new(1);
        let calls = Cell::new(0);
        let result = run_deferred(
            slots.try_acquire().unwrap(),
            async { Err(()) },
            async {
                calls.set(calls.get() + 1);
                7
            },
            |_| async { panic!("must not edit after failed defer") },
        )
        .await;
        assert_eq!(result, Err(DeliveryError::Defer));
        assert_eq!(calls.get(), 0);
        assert_eq!(slots.available_permits(), 1);
    }

    #[tokio::test]
    async fn provider_runs_after_defer_and_slot_is_released_before_failed_edit() {
        let slots = Semaphore::new(1);
        let deferred = Cell::new(false);
        let calls = Cell::new(0);
        let edits = Cell::new(0);
        let result = run_deferred(
            slots.try_acquire().unwrap(),
            async {
                deferred.set(true);
                Ok(())
            },
            async {
                assert!(deferred.get());
                assert_eq!(slots.available_permits(), 0);
                calls.set(calls.get() + 1);
                7
            },
            |value| {
                assert_eq!(value, 7);
                assert_eq!(slots.available_permits(), 1);
                edits.set(edits.get() + 1);
                std::future::ready(Err(()))
            },
        )
        .await;
        assert_eq!(result, Err(DeliveryError::Edit));
        assert_eq!(calls.get(), 1);
        assert_eq!(edits.get(), 1);
    }

    #[tokio::test]
    async fn provider_failure_is_delivered_once_and_releases_permit() {
        let slots = Semaphore::new(1);
        let edits = Cell::new(0);
        run_deferred(
            slots.try_acquire().unwrap(),
            async { Ok(()) },
            async { Err::<(), _>("timeout") },
            |value| {
                assert_eq!(value, Err("timeout"));
                assert_eq!(slots.available_permits(), 1);
                edits.set(edits.get() + 1);
                std::future::ready(Ok(()))
            },
        )
        .await
        .unwrap();
        assert_eq!(edits.get(), 1);
    }

    #[tokio::test]
    async fn cancellation_during_provider_wait_releases_permit_without_editing() {
        let slots = Semaphore::new(1);
        let calls = Cell::new(0);
        let mut operation = Box::pin(run_deferred(
            slots.try_acquire().unwrap(),
            async { Ok(()) },
            async {
                calls.set(1);
                pending::<()>().await
            },
            |_| async { panic!("cancelled evaluation must not edit") },
        ));
        tokio::select! {
            biased;
            _ = &mut operation => panic!("evaluation should be pending"),
            _ = tokio::task::yield_now() => {}
        }
        assert_eq!(calls.get(), 1);
        assert_eq!(slots.available_permits(), 0);
        drop(operation);
        assert_eq!(slots.available_permits(), 1);
    }
}
