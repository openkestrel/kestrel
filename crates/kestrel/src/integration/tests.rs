use anyhow::anyhow;
use jiff::SignedDuration;

use super::back_off;
use super::github::Refused;
use super::repository::same_repository;

#[test]
fn a_repository_is_the_same_whichever_form_git_or_gh_names_it_in() {
    let selected = "https://github.com/jtmthf/kestrel.git";

    for asked in [
        "https://github.com/jtmthf/kestrel.git",
        "https://github.com/jtmthf/kestrel",
        "https://github.com/JTMTHF/Kestrel/",
    ] {
        assert!(same_repository(selected, asked), "{asked}");
    }
    for asked in [
        "http://github.com/jtmthf/kestrel.git",
        "https://github.com/jtmthf/kestrel-fork.git",
        "https://example.com/jtmthf/kestrel.git",
        "git@github.com:jtmthf/kestrel.git",
    ] {
        assert!(!same_repository(selected, asked), "{asked}");
    }
}

#[test]
fn a_rate_limit_defers_the_next_poll_to_the_moment_it_lifts() {
    let until = crate::timer::Clock::System.now() + SignedDuration::from_hours(1);

    assert_eq!(
        back_off(
            SignedDuration::from_secs(1),
            &Refused::RateLimited { until },
            crate::timer::Clock::System.now(),
        ),
        until
    );
}

#[test]
fn a_rate_limit_that_has_already_lifted_still_waits_the_interval_out() {
    let until = crate::timer::Clock::System.now() - SignedDuration::from_hours(1);

    assert!(
        back_off(
            SignedDuration::from_secs(60),
            &Refused::RateLimited { until },
            crate::timer::Clock::System.now(),
        ) > crate::timer::Clock::System.now()
    );
}

#[test]
fn a_failure_waits_the_interval_out_rather_than_hammering() {
    assert!(
        back_off(
            SignedDuration::from_secs(60),
            &Refused::Failed(anyhow!("connection refused")),
            crate::timer::Clock::System.now(),
        ) > crate::timer::Clock::System.now() + SignedDuration::from_secs(50)
    );
}
