use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

const CAPACITY: u32 = 20;
const REFILL: Duration = Duration::from_secs(5);

pub(crate) const LOGIN_ERROR: &str = "Please try later";
const FAILURE_CAPACITY: usize = 4096;
const FAILURE_TTL_MINUTES: i32 = 10;

pub(crate) fn prepare_failure_entry(
    failures: &mut HashMap<String, (i32, i32, i32)>,
    key: &str,
    minute: i32,
) {
    if failures
        .get(key)
        .is_some_and(|entry| minute.saturating_sub(entry.0) >= FAILURE_TTL_MINUTES)
    {
        failures.remove(key);
    }
    if !failures.contains_key(key) && failures.len() >= FAILURE_CAPACITY {
        failures.retain(|_, entry| minute.saturating_sub(entry.0) < FAILURE_TTL_MINUTES);
        if failures.len() >= FAILURE_CAPACITY {
            if let Some(oldest) = failures
                .iter()
                .min_by_key(|(_, entry)| entry.0)
                .map(|(key, _)| key.clone())
            {
                failures.remove(&oldest);
            }
        }
    }
}

struct PasswordBudget {
    tokens: u32,
    replenished: Instant,
}

impl PasswordBudget {
    fn new(now: Instant) -> Self {
        Self {
            tokens: CAPACITY,
            replenished: now,
        }
    }

    fn take(&mut self, now: Instant) -> bool {
        let elapsed = now.saturating_duration_since(self.replenished);
        let refilled = elapsed.as_nanos() / REFILL.as_nanos();
        if refilled > 0 {
            self.tokens = self
                .tokens
                .saturating_add(refilled.min(CAPACITY as u128) as u32)
                .min(CAPACITY);
            self.replenished =
                now - Duration::from_nanos((elapsed.as_nanos() % REFILL.as_nanos()) as u64);
        }
        if self.tokens == 0 {
            return false;
        }
        self.tokens -= 1;
        true
    }
}

hbb_common::lazy_static::lazy_static! {
    static ref PASSWORD_BUDGET: Mutex<PasswordBudget> = Mutex::new(PasswordBudget::new(Instant::now()));
}

pub(crate) fn allow_password_attempt() -> bool {
    PASSWORD_BUDGET.lock().unwrap().take(Instant::now())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn rotating_identities_share_one_finite_password_budget() {
        let now = Instant::now();
        let mut budget = PasswordBudget::new(now);
        let accepted = (0..10_000).filter(|_| budget.take(now)).count();
        assert_eq!(accepted, CAPACITY as usize);
        assert!(!budget.take(now + REFILL - Duration::from_nanos(1)));
        assert!(budget.take(now + REFILL));
        assert!(!budget.take(now + REFILL));
    }

    #[test]
    fn budget_recovers_without_reset_and_never_accumulates_excess_credit() {
        let now = Instant::now();
        let mut budget = PasswordBudget::new(now);
        for _ in 0..CAPACITY {
            assert!(budget.take(now));
        }
        let later = now + Duration::from_secs(3600);
        for _ in 0..CAPACITY {
            assert!(budget.take(later));
        }
        assert!(!budget.take(later));
        assert!(budget.take(later + REFILL));
        assert!(!budget.take(later + REFILL));
    }

    #[test]
    fn concurrent_password_checks_cannot_overspend() {
        let now = Instant::now();
        let budget = Arc::new(Mutex::new(PasswordBudget::new(now)));
        let checks: Vec<_> = (0..64)
            .map(|_| {
                let budget = budget.clone();
                std::thread::spawn(move || budget.lock().unwrap().take(now))
            })
            .collect();
        let accepted = checks
            .into_iter()
            .map(|check| u32::from(check.join().unwrap()))
            .sum::<u32>();
        assert_eq!(accepted, CAPACITY);
    }

    #[test]
    fn identity_churn_cannot_grow_failure_records_without_bound() {
        let mut failures = HashMap::new();
        for peer in 0..FAILURE_CAPACITY + 100 {
            let key = peer.to_string();
            prepare_failure_entry(&mut failures, &key, 100);
            failures.insert(key, (100, 1, 1));
            assert!(failures.len() <= FAILURE_CAPACITY);
        }
        prepare_failure_entry(&mut failures, "new", 110);
        assert!(failures.is_empty());
    }

    #[test]
    fn failure_cache_does_not_clear_current_blocks_on_reconnect() {
        let mut failures = HashMap::from([("peer".to_owned(), (100, 7, 31))]);
        prepare_failure_entry(&mut failures, "peer", 109);
        assert_eq!(failures["peer"], (100, 7, 31));
        prepare_failure_entry(&mut failures, "peer", 110);
        assert!(!failures.contains_key("peer"));
    }
}
