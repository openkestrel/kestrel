use std::num::NonZeroUsize;
use std::path::Path;
use std::time::Duration;

use kestrel::capability::Images;
use kestrel::compute::{Docker, Driver, LocalExec};
use kestrel::role::serve::{self, Listen};
use kestrel::role::work::Dispatch;
use kestrel::sign_in::check::Providers;
use kestrel::store::Store;
use kestrel::timer::Wake;
use tempfile::TempDir;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use super::super::{
    Cleanup, DEFAULT_INTERRUPT_DEADLINE, HARNESS, Kestrel, LOOPBACK, PROVIDER_DEADLINE, Provisions,
    QUIET_PERIOD, SERIALIZED, client_url, no_provider, scripted_agent, spawning,
};

impl Kestrel {
    /// Boots with no supervisor to provision an Environment with, so the work role claims
    /// nothing and a test is the only thing dispatching the Sessions it opens.
    pub async fn boot() -> Self {
        Self::booted(None).await
    }

    pub async fn boot_with_follow_lease(follow_lease: Duration) -> Self {
        Self::booted_with(None, follow_lease).await
    }

    /// Bound on every interface rather than on loopback, because what dials this one is a
    /// container and reaches this machine by its gateway address.
    pub async fn boot_reachable_from_an_environment() -> Self {
        let data_dir = TempDir::new().expect("a temporary data directory");
        Self::boot_against(
            data_dir,
            Listen {
                link: "0.0.0.0:0".parse().expect("every interface"),
                operator: LOOPBACK,
            },
            None,
            kestrel::presence::LEASE,
        )
        .await
    }

    pub async fn boot_with_the_operator_beyond_loopback() -> Self {
        let data_dir = TempDir::new().expect("a temporary data directory");
        Self::boot_against(
            data_dir,
            Listen {
                link: LOOPBACK,
                operator: "0.0.0.0:0".parse().expect("every interface"),
            },
            None,
            kestrel::presence::LEASE,
        )
        .await
    }

    pub async fn dispatching(supervisor: &Path) -> Self {
        Self::dispatching_to(
            supervisor,
            &scripted_agent::playing(scripted_agent::Script::Speaks),
        )
        .await
    }

    pub async fn dispatching_to(supervisor: &Path, command: &str) -> Self {
        Self::dispatching_up_to(supervisor, command, 2).await
    }

    pub async fn dispatching_up_to(supervisor: &Path, command: &str, maximum: usize) -> Self {
        Self::dispatching_harnesses_up_to(supervisor, &[(HARNESS, command)], maximum).await
    }

    pub async fn dispatching_harnesses(supervisor: &Path, harnesses: &[(&str, &str)]) -> Self {
        Self::dispatching_harnesses_up_to(supervisor, harnesses, 2).await
    }

    pub async fn dispatching_harnesses_up_to(
        supervisor: &Path,
        harnesses: &[(&str, &str)],
        maximum: usize,
    ) -> Self {
        Self::booted(Some(Provisions {
            driver: Driver::LocalExec(LocalExec::running(supervisor)),
            harnesses: spawning(harnesses),
            max_active_sessions: NonZeroUsize::new(maximum).expect("at least one active session"),
            interrupt_deadline: DEFAULT_INTERRUPT_DEADLINE,
            sign_in_test_deadline: kestrel::sign_in::model_test::DEADLINE,
        }))
        .await
    }

    /// A sign-in test here is given up on after `deadline`, not the two minutes a real one gets.
    pub async fn dispatching_harnesses_testing_sign_ins_within(
        supervisor: &Path,
        harnesses: &[(&str, &str)],
        deadline: Duration,
    ) -> Self {
        Self::booted(Some(Provisions {
            driver: Driver::LocalExec(LocalExec::running(supervisor)),
            harnesses: spawning(harnesses),
            max_active_sessions: NonZeroUsize::new(2).expect("at least one active session"),
            interrupt_deadline: DEFAULT_INTERRUPT_DEADLINE,
            sign_in_test_deadline: deadline,
        }))
        .await
    }

    pub async fn dispatching_with_a_quick_interrupt(
        supervisor: &Path,
        command: &str,
        deadline: Duration,
    ) -> Self {
        Self::booted(Some(Provisions {
            driver: Driver::LocalExec(LocalExec::running(supervisor)),
            harnesses: spawning(&[(HARNESS, command)]),
            max_active_sessions: NonZeroUsize::new(2).expect("at least one active session"),
            interrupt_deadline: deadline,
            sign_in_test_deadline: kestrel::sign_in::model_test::DEADLINE,
        }))
        .await
    }

    pub async fn dispatching_in(image: &str, command: &str) -> Self {
        Self::dispatching_harnesses_in(image, &[(HARNESS, command)]).await
    }

    /// The Docker driver, on a control plane bound where a container can dial out to it.
    pub async fn dispatching_harnesses_in(image: &str, harnesses: &[(&str, &str)]) -> Self {
        let data_dir = TempDir::new().expect("a temporary data directory");
        Self::boot_against(
            data_dir,
            Listen {
                link: "0.0.0.0:0".parse().expect("every interface"),
                operator: LOOPBACK,
            },
            Some(Provisions {
                driver: Driver::Docker(Docker::provisioning_from(image)),
                harnesses: spawning(harnesses),
                max_active_sessions: NonZeroUsize::new(2).unwrap(),
                interrupt_deadline: DEFAULT_INTERRUPT_DEADLINE,
                sign_in_test_deadline: kestrel::sign_in::model_test::DEADLINE,
            }),
            kestrel::presence::LEASE,
        )
        .await
    }

    async fn booted(environment: Option<Provisions>) -> Self {
        Self::booted_with(environment, kestrel::presence::LEASE).await
    }

    async fn booted_with(environment: Option<Provisions>, follow_lease: Duration) -> Self {
        let data_dir = TempDir::new().expect("a temporary data directory");
        Self::boot_against(
            data_dir,
            Listen {
                link: LOOPBACK,
                operator: LOOPBACK,
            },
            environment,
            follow_lease,
        )
        .await
    }

    /// Checks API keys against `providers`, a stub's address, never a real provider's.
    pub async fn boot_checking_sign_ins_at(providers: &str) -> Self {
        Self::boot_checking(
            TempDir::new().expect("a temporary data directory"),
            Listen {
                link: LOOPBACK,
                operator: LOOPBACK,
            },
            None,
            kestrel::presence::LEASE,
            Providers::at(providers, providers, PROVIDER_DEADLINE),
            client_url(),
            Images::default(),
        )
        .await
    }

    pub async fn boot_with_the_client_at(client: &str) -> Self {
        Self::boot_serving(client.parse().expect("a Client URL"), Images::default()).await
    }

    /// Dispatches nothing, so the image is only ever inspected.
    pub async fn boot_inspecting(image: &str) -> Self {
        Self::boot_serving(client_url(), Images::inspecting(image)).await
    }

    async fn boot_serving(client: url::Url, images: Images) -> Self {
        Self::boot_checking(
            TempDir::new().expect("a temporary data directory"),
            Listen {
                link: LOOPBACK,
                operator: LOOPBACK,
            },
            None,
            kestrel::presence::LEASE,
            no_provider(),
            client,
            images,
        )
        .await
    }

    pub(in super::super) async fn boot_against(
        data_dir: TempDir,
        listen: Listen,
        environment: Option<Provisions>,
        follow_lease: Duration,
    ) -> Self {
        Self::boot_checking(
            data_dir,
            listen,
            environment,
            follow_lease,
            no_provider(),
            client_url(),
            Images::default(),
        )
        .await
    }

    async fn boot_checking(
        data_dir: TempDir,
        listen: Listen,
        environment: Option<Provisions>,
        follow_lease: Duration,
        providers: Providers,
        client: url::Url,
        images: Images,
    ) -> Self {
        let store = Store::open(data_dir.path())
            .await
            .expect("the control plane should boot against a fresh data directory");
        let shutdown = CancellationToken::new();
        let all_in_one = kestrel::role::bind(store.clone(), listen, client, follow_lease)
            .await
            .expect("the control plane should bind its link")
            .checking_sign_ins_with(providers)
            .inspecting(images);
        let bound = all_in_one.bound();
        let address = bound.link;
        let dispatch = environment.clone().map(|provisions| Dispatch {
            link: match provisions.driver {
                Driver::Docker(_) => format!("http://host.docker.internal:{}", address.port()),
                Driver::LocalExec(_) => format!("http://{address}"),
            },
            driver: provisions.driver,
            harnesses: provisions.harnesses,
            auth: None,
            max_active_sessions: provisions.max_active_sessions,
            serialized: vec![SERIALIZED.to_owned()],
            interrupt_deadline: provisions.interrupt_deadline,
            quiet_period: Some(QUIET_PERIOD),
            sign_in_test_deadline: provisions.sign_in_test_deadline,
        });
        let roles = tokio::spawn(all_in_one.run(dispatch, shutdown.clone()));

        Self::running(
            data_dir,
            store,
            bound,
            environment,
            shutdown,
            roles,
            follow_lease,
        )
    }

    /// Serves the link with no work role behind it, so nothing sweeps a lease a test has let
    /// lapse until it restarts as a whole control plane.
    pub async fn boot_serving_alone() -> Self {
        Self::boot_serving_at_github(kestrel::integration::github::API).await
    }

    pub async fn boot_serving_at_github(api: &str) -> Self {
        let data_dir = TempDir::new().expect("a temporary data directory");
        let store = Store::open(data_dir.path())
            .await
            .expect("the control plane should boot against a fresh data directory");
        let shutdown = CancellationToken::new();
        let listening = serve::bind(
            store.clone(),
            Listen {
                link: LOOPBACK,
                operator: LOOPBACK,
            },
            client_url(),
            Wake::default(),
            kestrel::presence::LEASE,
        )
        .await
        .expect("the control plane should bind its link")
        .checking_sign_ins_with(no_provider())
        .creating_github_apps_at(api);
        let bound = listening.bound();
        let roles = tokio::spawn(serve::run(listening, shutdown.clone()));

        Self::running(
            data_dir,
            store,
            bound,
            None,
            shutdown,
            roles,
            kestrel::presence::LEASE,
        )
    }

    fn running(
        data_dir: TempDir,
        store: Store,
        bound: Listen,
        environment: Option<Provisions>,
        shutdown: CancellationToken,
        roles: JoinHandle<anyhow::Result<()>>,
        follow_lease: Duration,
    ) -> Self {
        let cleanup = Cleanup {
            data_dir: data_dir.path().to_path_buf(),
            environment: environment.clone(),
            shutdown: shutdown.clone(),
            abort: roles.abort_handle(),
            armed: true,
        };

        Self {
            cleanup,
            data_dir,
            store,
            bound,
            environment,
            shutdown,
            roles,
            follow_lease,
        }
    }
}
