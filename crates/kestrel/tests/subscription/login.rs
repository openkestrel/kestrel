use std::fs;

use super::secrets::Secrets;
use super::{ORGANIZATION, PROFILE, Subject};
use crate::support::Kestrel;
use kestrel::profile::Contents;

/// Everything the profile held and holds now, because a login refreshed mid-smoke is as much a
/// secret as the one it replaced.
async fn held_now(kestrel: &Kestrel) -> Contents {
    let profile = kestrel
        .profiles(ORGANIZATION)
        .await
        .into_iter()
        .map(|(profile, _)| profile)
        .find(|profile| profile.name == PROFILE)
        .expect("the smoke's profile");

    kestrel.profile_contents(&profile).await
}

pub(super) async fn secrets(kestrel: &Kestrel, subject: &Subject) -> Secrets {
    let now = held_now(kestrel).await;
    let held = subject
        .variables
        .iter()
        .map(|(_, value)| value.as_str())
        .chain(subject.files.iter().map(|login| login.as_read.as_str()))
        .chain(now.variables.values().map(String::as_str))
        .chain(now.files.values().map(String::as_str));

    Secrets::of(&held.collect::<Vec<_>>())
}

pub(super) async fn handed_back(kestrel: &Kestrel, subject: &Subject) {
    let now = held_now(kestrel).await;

    for login in &subject.files {
        let Some(refreshed) = now.files.get(login.path) else {
            continue;
        };
        if *refreshed == login.as_read {
            continue;
        }
        let host = &login.host;
        if fs::read_to_string(host).ok().as_ref() != Some(&login.as_read) {
            eprintln!(
                "{} changed on the host during the smoke, so the login the harness refreshed is not written over it",
                host.display()
            );
            continue;
        }
        let beside = host.with_extension("kestrel-smoke");
        write_privately(&beside, refreshed);
        fs::rename(&beside, host).expect("the refreshed login should replace the one read");
        eprintln!("handed the refreshed login back to {}", host.display());
    }
}

fn write_privately(path: &std::path::Path, contents: &str) {
    use std::io::Write as _;

    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options
        .open(path)
        .and_then(|mut file| file.write_all(contents.as_bytes()))
        .expect("the refreshed login should write");
}
